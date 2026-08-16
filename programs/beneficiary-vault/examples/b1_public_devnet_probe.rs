// SPDX-License-Identifier: MIT OR Apache-2.0
//! Install the exact B1 SBF on a public Solana cluster through the upgradeable
//! loader, exercise the custody path, and leave behind state that a stranger
//! can reconstruct from public accounts alone.
//!
//! This probe deliberately cannot do three things. It cannot reach a cluster
//! whose genesis hash was not declared in advance; it cannot touch mainnet at
//! all; and it cannot send a release transaction, because the frozen 730-day
//! covenant cliff is not crossable on a public cluster and no shortened-cliff
//! build exists. What it does establish is that the Program and ProgramData
//! accounts were created by real loader transactions at an address whose signer
//! is retained, that ProgramData holds the exact SBF bytes, that the deposit
//! moved custody, that the on-chain monthly cap is the expected value, and that
//! a pre-cliff release is refused with `CliffActive`.
//!
//! Signers are read from keypair files outside every Git repository. No private
//! key is ever serialized into output.

use anchor_lang::{
    solana_program::bpf_loader_upgradeable::{
        self, deploy_with_max_program_len, get_program_data_address, UpgradeableLoaderState,
    },
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use beneficiary_vault::{accounts, instruction, state::BeneficiaryVault};
use serde_json::json;
use sha2::{Digest, Sha256};
use solana_commitment_config::CommitmentConfig;
use solana_instruction::Instruction;
use solana_keypair::{read_keypair_file, Keypair};
use solana_program_pack::Pack;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSimulateTransactionConfig;
use solana_signer::Signer;
use solana_system_interface::{instruction as system_instruction, program as system_program};
use solana_transaction::Transaction;
use spl_token_interface::{
    instruction as token_instruction,
    state::{Account as SplAccount, Mint},
    ID as TOKEN_PROGRAM_ID,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

/// Never permitted, whatever the operator declares.
const MAINNET_GENESIS: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";

const DEPOSIT: u64 = 1_000_000_000;
const ANNUAL_BPS: u16 = 500;
const EXPECTED_MONTHLY_CAP: u64 = 4_166_666;
const CLIFF_SECONDS: i64 = 730 * 24 * 60 * 60;
const POLICY_HASH: [u8; 32] = [0x42; 32];
const WRITE_CHUNK_BYTES: usize = 900;

/// Documented public-endpoint limits are 40 requests per 10 s per IP for a
/// single RPC method. Three sends per second stays well inside that.
const SEND_INTERVAL: Duration = Duration::from_millis(334);
const MAX_ATTEMPTS_PER_CHUNK: usize = 5;
const MAX_TOTAL_CHUNK_RESENDS: usize = 40;
/// A public-cluster blockhash stays valid for roughly 150 slots; refresh well
/// inside that rather than after a fixed number of writes.
const BLOCKHASH_MAX_AGE: Duration = Duration::from_secs(20);
/// Hard spend ceiling for the whole run, in lamports (2.5 SOL).
const SPEND_CEILING_LAMPORTS: u64 = 2_500_000_000;

struct Ctx {
    rpc: RpcClient,
    rpc_url: String,
    confirmed_stages: Vec<String>,
    last_send: Option<Instant>,
}

impl Ctx {
    fn gate(&mut self, stage: &str) -> Result<(), Box<dyn std::error::Error>> {
        if !self.confirmed_stages.iter().any(|value| value == stage) {
            return Err(format!(
                "stage {stage} is not listed in K4V_DEVNET_STAGE_CONFIRM; nothing was sent"
            )
            .into());
        }
        Ok(())
    }

    fn pace(&mut self) {
        if let Some(last) = self.last_send {
            let elapsed = last.elapsed();
            if elapsed < SEND_INTERVAL {
                thread::sleep(SEND_INTERVAL - elapsed);
            }
        }
        self.last_send = Some(Instant::now());
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn program_path() -> PathBuf {
    repository_root().join("target/deploy/beneficiary_vault.so")
}

/// Stop condition 3: a signer file must not live inside any Git repository.
fn assert_outside_git(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut cursor = fs::canonicalize(path)?;
    while let Some(parent) = cursor.parent() {
        if parent.join(".git").exists() {
            return Err(format!(
                "{} is inside the Git repository at {}; refusing to use it",
                path.display(),
                parent.display()
            )
            .into());
        }
        cursor = parent.to_path_buf();
    }
    Ok(())
}

fn load_signer(variable: &str) -> Result<Keypair, Box<dyn std::error::Error>> {
    let raw = env::var(variable).map_err(|_| format!("{variable} is not set"))?;
    let path = PathBuf::from(shellexpand_home(&raw));
    assert_outside_git(&path)?;
    read_keypair_file(&path).map_err(|error| format!("{variable}: {error}").into())
}

fn shellexpand_home(value: &str) -> String {
    match value.strip_prefix("~/") {
        Some(rest) => match env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => value.to_string(),
        },
        None => value.to_string(),
    }
}

fn simulate(
    ctx: &Ctx,
    tx: &Transaction,
    stage: &str,
    expected_log: Option<&str>,
) -> Result<u64, Box<dyn std::error::Error>> {
    let result = ctx
        .rpc
        .simulate_transaction_with_config(
            tx,
            RpcSimulateTransactionConfig {
                sig_verify: true,
                commitment: Some(CommitmentConfig::confirmed()),
                ..RpcSimulateTransactionConfig::default()
            },
        )?
        .value;
    let units = result.units_consumed.unwrap_or_default();
    let logs = result.logs.unwrap_or_default();
    match expected_log {
        None => {
            if let Some(error) = result.err {
                return Err(format!("{stage} simulation failed: {error:?}; logs={logs:?}").into());
            }
        }
        Some(needle) => {
            if result.err.is_none() || !logs.iter().any(|line| line.contains(needle)) {
                return Err(format!(
                    "{stage} simulation did not produce the expected {needle}: err={:?}; logs={logs:?}",
                    result.err
                )
                .into());
            }
        }
    }
    Ok(units)
}

fn signed(
    ctx: &Ctx,
    instructions: &[Instruction],
    payer: &Keypair,
    extra: &[&Keypair],
) -> Result<Transaction, Box<dyn std::error::Error>> {
    let mut signers: Vec<&Keypair> = Vec::with_capacity(1 + extra.len());
    signers.push(payer);
    signers.extend_from_slice(extra);
    Ok(Transaction::new_signed_with_payer(
        instructions,
        Some(&payer.pubkey()),
        &signers,
        ctx.rpc.get_latest_blockhash()?,
    ))
}

fn simulate_then_send(
    ctx: &mut Ctx,
    tx: &Transaction,
    stage: &str,
) -> Result<(String, u64), Box<dyn std::error::Error>> {
    let units = simulate(ctx, tx, stage, None)?;
    ctx.gate(stage)?;
    ctx.pace();
    let signature = ctx.rpc.send_and_confirm_transaction_with_spinner(tx)?;
    println!("SENT {stage} signature={signature} units={units}");
    Ok((signature.to_string(), units))
}

struct Deployment {
    buffer_create_signature: String,
    first_write_signature: String,
    last_write_signature: String,
    deploy_signature: String,
    write_transactions: usize,
    chunk_resends: usize,
    programdata_address: Pubkey,
    programdata_slot: u64,
    sbf_sha256: String,
}

#[allow(clippy::too_many_arguments)]
fn deploy(
    ctx: &mut Ctx,
    payer: &Keypair,
    program: &Keypair,
    buffer: &Keypair,
    authority: &Keypair,
    bytes: &[u8],
) -> Result<Deployment, Box<dyn std::error::Error>> {
    let buffer_lamports =
        ctx.rpc
            .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_buffer(
                bytes.len(),
            ))?;
    let create = bpf_loader_upgradeable::create_buffer(
        &payer.pubkey(),
        &buffer.pubkey(),
        &authority.pubkey(),
        buffer_lamports,
        bytes.len(),
    )?;
    let tx = signed(ctx, &create, payer, &[buffer])?;
    let (buffer_create_signature, _) = simulate_then_send(ctx, &tx, "LOADER_CREATE_BUFFER")?;

    ctx.gate("LOADER_WRITE_CHUNKS")?;
    let write_transactions = bytes.len().div_ceil(WRITE_CHUNK_BYTES);
    let mut first_write_signature = None;
    let mut last_write_signature = None;
    let mut chunk_resends = 0usize;
    let mut blockhash = ctx.rpc.get_latest_blockhash()?;
    let mut blockhash_age = Instant::now();
    for (index, chunk) in bytes.chunks(WRITE_CHUNK_BYTES).enumerate() {
        // Refresh by age, not by count. A public cluster confirms each write in
        // seconds rather than milliseconds, so a fixed every-N-writes refresh
        // lets the cached blockhash expire mid-window and turns ordinary
        // latency into avoidable retries.
        if blockhash_age.elapsed() >= BLOCKHASH_MAX_AGE {
            blockhash = ctx.rpc.get_latest_blockhash()?;
            blockhash_age = Instant::now();
        }
        let offset = u32::try_from(index * WRITE_CHUNK_BYTES)?;
        let instruction = bpf_loader_upgradeable::write(
            &buffer.pubkey(),
            &authority.pubkey(),
            offset,
            chunk.to_vec(),
        );
        let mut attempt = 0usize;
        let signature = loop {
            attempt += 1;
            let tx = Transaction::new_signed_with_payer(
                std::slice::from_ref(&instruction),
                Some(&payer.pubkey()),
                &[payer, authority],
                blockhash,
            );
            ctx.pace();
            match ctx.rpc.send_and_confirm_transaction(&tx) {
                Ok(signature) => break signature.to_string(),
                Err(error) => {
                    if attempt >= MAX_ATTEMPTS_PER_CHUNK {
                        return Err(format!(
                            "loader write {index} failed after {attempt} attempts: {error}"
                        )
                        .into());
                    }
                    chunk_resends += 1;
                    if chunk_resends > MAX_TOTAL_CHUNK_RESENDS {
                        return Err(format!(
                            "loader writes exceeded the {MAX_TOTAL_CHUNK_RESENDS} total resend cap"
                        )
                        .into());
                    }
                    eprintln!("RETRY write {index} attempt {attempt}: {error}");
                    blockhash = ctx.rpc.get_latest_blockhash()?;
                    blockhash_age = Instant::now();
                }
            }
        };
        first_write_signature.get_or_insert_with(|| signature.clone());
        last_write_signature = Some(signature);
        if (index + 1) % 32 == 0 || index + 1 == write_transactions {
            println!(
                "LOADER_WRITE_PROGRESS {}/{write_transactions} resends={chunk_resends}",
                index + 1
            );
        }
    }

    let program_lamports = ctx
        .rpc
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program())?;
    let deploy_instructions = deploy_with_max_program_len(
        &payer.pubkey(),
        &program.pubkey(),
        &buffer.pubkey(),
        &authority.pubkey(),
        program_lamports,
        bytes.len(),
    )?;
    if deploy_instructions.len() != 2 {
        return Err("unexpected loader deploy instruction count".into());
    }
    let tx = signed(ctx, &deploy_instructions, payer, &[program, authority])?;
    let (deploy_signature, _) = simulate_then_send(ctx, &tx, "LOADER_DEPLOY")?;

    let program_account = ctx.rpc.get_account(&program.pubkey())?;
    if program_account.owner != bpf_loader_upgradeable::ID || !program_account.executable {
        return Err("the deployed Program account is not an executable loader account".into());
    }
    let programdata_address = match bincode::deserialize(&program_account.data)? {
        UpgradeableLoaderState::Program {
            programdata_address,
        } => programdata_address,
        other => return Err(format!("unexpected Program state: {other:?}").into()),
    };
    if programdata_address != get_program_data_address(&program.pubkey()) {
        return Err("ProgramData PDA did not reconcile".into());
    }
    let programdata = ctx.rpc.get_account(&programdata_address)?;
    if programdata.owner != bpf_loader_upgradeable::ID || programdata.executable {
        return Err("ProgramData owner or executable flag was invalid".into());
    }
    let metadata_len = UpgradeableLoaderState::size_of_programdata_metadata();
    if programdata.data.len() != metadata_len + bytes.len()
        || &programdata.data[metadata_len..] != bytes
    {
        return Err("ProgramData bytes did not equal the exact B1 SBF".into());
    }
    let programdata_slot = match bincode::deserialize(&programdata.data[..metadata_len])? {
        UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address,
        } if upgrade_authority_address == Some(authority.pubkey()) => slot,
        other => return Err(format!("unexpected ProgramData state: {other:?}").into()),
    };

    Ok(Deployment {
        buffer_create_signature,
        first_write_signature: first_write_signature.ok_or("no loader write transaction")?,
        last_write_signature: last_write_signature.ok_or("no loader write transaction")?,
        deploy_signature,
        write_transactions,
        chunk_resends,
        programdata_address,
        programdata_slot,
        sbf_sha256: hex::encode(Sha256::digest(bytes)),
    })
}

/// Export the public state with the read-only RPC exporter, then hand the
/// resulting snapshot to the standalone verifier.
///
/// Two separate programs are used on purpose. The exporter already refuses to
/// print a snapshot it cannot verify, so a zero exit status is itself a
/// verdict; but a public claim needs the verifier's explicit receipt — `valid`,
/// `reasons`, the recomputed cap and the surplus — rather than an exit code
/// that a reader has to take on trust.
fn run_independent_verifier(
    rpc_url: &str,
    program_id: &Pubkey,
    vault_state: &Pubkey,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let export = Command::new("python3")
        .current_dir(repository_root())
        .env("PYTHONPATH", "src")
        .arg("src/beneficiary_vault_rpc_exporter.py")
        .arg("--rpc-url")
        .arg(rpc_url)
        .arg("--program-id")
        .arg(program_id.to_string())
        .arg("--vault-state")
        .arg(vault_state.to_string())
        .output()?;
    if !export.status.success() {
        return Err(format!(
            "independent RPC export failed: {}",
            String::from_utf8_lossy(&export.stderr)
        )
        .into());
    }

    let snapshot_path = env::temp_dir().join(format!("k4v_public_snapshot_{vault_state}.json"));
    fs::write(&snapshot_path, &export.stdout)?;
    let verify = Command::new("python3")
        .current_dir(repository_root())
        .env("PYTHONPATH", "src")
        .arg("src/beneficiary_vault_verifier.py")
        .arg(&snapshot_path)
        .output();
    let _ = fs::remove_file(&snapshot_path);
    let verify = verify?;
    if !verify.status.success() {
        return Err(format!(
            "independent verifier rejected the public snapshot: {}",
            String::from_utf8_lossy(&verify.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&verify.stdout)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::var("K4V_DEVNET_PROBE").as_deref() != Ok("1") {
        return Err("set K4V_DEVNET_PROBE=1 to acknowledge a public-cluster run".into());
    }
    let bytes = fs::read(program_path())
        .map_err(|_| "build the B1 SBF into target/deploy first".to_string())?;
    let sbf_sha256 = hex::encode(Sha256::digest(&bytes));
    let expected_sbf = env::var("K4V_EXPECTED_SBF_SHA256")
        .map_err(|_| "K4V_EXPECTED_SBF_SHA256 is not set".to_string())?;
    if sbf_sha256 != expected_sbf {
        return Err(
            format!("local SBF {sbf_sha256} does not equal the frozen {expected_sbf}").into(),
        );
    }

    let rpc_url =
        env::var("K4V_DEVNET_RPC").unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());
    let expected_genesis = env::var("K4V_EXPECTED_GENESIS")
        .map_err(|_| "K4V_EXPECTED_GENESIS is not set; declare the cluster before connecting")?;
    if expected_genesis == MAINNET_GENESIS {
        return Err("mainnet is refused unconditionally".into());
    }
    let confirmed_stages: Vec<String> = env::var("K4V_DEVNET_STAGE_CONFIRM")
        .unwrap_or_default()
        .split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();

    let is_loopback = rpc_url.contains("127.0.0.1") || rpc_url.contains("localhost");
    let rpc = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::confirmed());
    let genesis = rpc.get_genesis_hash()?.to_string();
    if genesis == MAINNET_GENESIS {
        return Err("the connected cluster is mainnet; refusing".into());
    }
    if genesis != expected_genesis {
        return Err(format!(
            "cluster genesis {genesis} does not equal the declared {expected_genesis}"
        )
        .into());
    }
    let mut ctx = Ctx {
        rpc,
        rpc_url: rpc_url.clone(),
        confirmed_stages,
        last_send: None,
    };
    println!(
        "CLUSTER rpc={rpc_url} genesis={genesis} version={:?}",
        ctx.rpc.get_version()?
    );

    let program = load_signer("K4V_PROGRAM_KEYPAIR")?;
    let authority = load_signer("K4V_UPGRADE_AUTHORITY_KEYPAIR")?;
    let payer = load_signer("K4V_PAYER_KEYPAIR")?;
    let beneficiary = load_signer("K4V_BENEFICIARY_KEYPAIR")?;
    if program.pubkey() != beneficiary_vault::ID {
        return Err(format!(
            "program keypair {} does not match the declared id {}",
            program.pubkey(),
            beneficiary_vault::ID
        )
        .into());
    }
    let buffer = Keypair::new();
    let mint = Keypair::new();
    let depositor_token = Keypair::new();
    let beneficiary_token = Keypair::new();

    if ctx
        .rpc
        .get_account_with_commitment(&program.pubkey(), CommitmentConfig::confirmed())?
        .value
        .is_some()
    {
        return Err("an account already exists at the declared program address".into());
    }
    let opening_balance = ctx.rpc.get_balance(&payer.pubkey())?;
    let required = ctx.rpc.get_minimum_balance_for_rent_exemption(
        UpgradeableLoaderState::size_of_programdata(bytes.len()),
    )? + 15_000_000;
    if opening_balance < required {
        return Err(format!(
            "payer {} holds {opening_balance} lamports, below the required {required}; \
             fund it with at most two faucet requests and rerun",
            payer.pubkey()
        )
        .into());
    }
    println!(
        "PREFLIGHT program={} payer={} balance={opening_balance} required={required}",
        program.pubkey(),
        payer.pubkey()
    );

    let deployment = deploy(&mut ctx, &payer, &program, &buffer, &authority, &bytes)?;
    println!(
        "DEPLOYED program={} programdata={} slot={} sha256={}",
        program.pubkey(),
        deployment.programdata_address,
        deployment.programdata_slot,
        deployment.sbf_sha256
    );

    let mint_rent = ctx.rpc.get_minimum_balance_for_rent_exemption(Mint::LEN)?;
    let token_rent = ctx
        .rpc
        .get_minimum_balance_for_rent_exemption(SplAccount::LEN)?;
    let setup_instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            mint_rent,
            Mint::LEN as u64,
            &TOKEN_PROGRAM_ID,
        ),
        token_instruction::initialize_mint2(
            &TOKEN_PROGRAM_ID,
            &mint.pubkey(),
            &payer.pubkey(),
            None,
            9,
        )?,
        system_instruction::create_account(
            &payer.pubkey(),
            &depositor_token.pubkey(),
            token_rent,
            SplAccount::LEN as u64,
            &TOKEN_PROGRAM_ID,
        ),
        token_instruction::initialize_account3(
            &TOKEN_PROGRAM_ID,
            &depositor_token.pubkey(),
            &mint.pubkey(),
            &payer.pubkey(),
        )?,
        system_instruction::create_account(
            &payer.pubkey(),
            &beneficiary_token.pubkey(),
            token_rent,
            SplAccount::LEN as u64,
            &TOKEN_PROGRAM_ID,
        ),
        token_instruction::initialize_account3(
            &TOKEN_PROGRAM_ID,
            &beneficiary_token.pubkey(),
            &mint.pubkey(),
            &beneficiary.pubkey(),
        )?,
        token_instruction::mint_to(
            &TOKEN_PROGRAM_ID,
            &mint.pubkey(),
            &depositor_token.pubkey(),
            &payer.pubkey(),
            &[],
            DEPOSIT,
        )?,
    ];
    let tx = signed(
        &ctx,
        &setup_instructions,
        &payer,
        &[&mint, &depositor_token, &beneficiary_token],
    )?;
    let (setup_signature, setup_units) = simulate_then_send(&mut ctx, &tx, "SETUP")?;

    let (vault_state, _) = Pubkey::find_program_address(
        &[
            b"beneficiary-vault",
            beneficiary.pubkey().as_ref(),
            mint.pubkey().as_ref(),
            POLICY_HASH.as_ref(),
        ],
        &program.pubkey(),
    );
    let (vault_token, _) = Pubkey::find_program_address(
        &[b"beneficiary-token", vault_state.as_ref()],
        &program.pubkey(),
    );
    let deposit_ix = Instruction {
        program_id: program.pubkey(),
        accounts: accounts::Deposit {
            depositor: payer.pubkey(),
            beneficiary: beneficiary.pubkey(),
            mint: mint.pubkey(),
            depositor_token: depositor_token.pubkey(),
            vault_state,
            vault_token,
            token_program: TOKEN_PROGRAM_ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: instruction::Deposit {
            amount: DEPOSIT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: CLIFF_SECONDS,
            policy_hash: POLICY_HASH,
        }
        .data(),
    };
    let tx = signed(&ctx, &[deposit_ix], &payer, &[])?;
    let (deposit_signature, deposit_units) = simulate_then_send(&mut ctx, &tx, "DEPOSIT")?;

    let state_account = ctx.rpc.get_account(&vault_state)?;
    if state_account.owner != program.pubkey() {
        return Err("vault state is not owned by the deployed program".into());
    }
    let state = BeneficiaryVault::try_deserialize(&mut state_account.data.as_slice())?;
    if state.monthly_cap != EXPECTED_MONTHLY_CAP {
        return Err(format!(
            "on-chain monthly cap {} does not equal the expected {EXPECTED_MONTHLY_CAP}",
            state.monthly_cap
        )
        .into());
    }
    if state.released_total != 0 {
        return Err("released_total must be zero before the cliff".into());
    }
    if state.cliff_end_ts - state.genesis_ts != CLIFF_SECONDS {
        return Err("stored cliff length does not equal the frozen minimum".into());
    }
    let vault_balance = SplAccount::unpack(&ctx.rpc.get_account(&vault_token)?.data)?.amount;
    if vault_balance != DEPOSIT {
        return Err(format!("vault holds {vault_balance}, expected {DEPOSIT}").into());
    }

    // Simulation only. The cliff is roughly two years away on a public cluster
    // and no release transaction is ever broadcast.
    let release_ix = Instruction {
        program_id: program.pubkey(),
        accounts: accounts::Release {
            beneficiary: beneficiary.pubkey(),
            vault_state,
            vault_token,
            beneficiary_token: beneficiary_token.pubkey(),
            mint: mint.pubkey(),
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::Release { amount: 1 }.data(),
    };
    let tx = signed(&ctx, &[release_ix], &payer, &[&beneficiary])?;
    let pre_cliff_units = simulate(&ctx, &tx, "PRE_CLIFF_RELEASE", Some("CliffActive"))?;
    println!("PRE_CLIFF_RELEASE refused as expected units={pre_cliff_units}");

    let receipt = run_independent_verifier(&ctx.rpc_url, &program.pubkey(), &vault_state)?;
    let verification = &receipt["verification"];
    let valid = verification["valid"].as_bool().unwrap_or(false);
    let reasons = verification["reasons"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(1);
    let surplus = verification["token_surplus_amount"].as_u64();
    let recomputed_cap = verification["expected_monthly_cap"].as_u64();
    if !valid || reasons != 0 || surplus != Some(0) || recomputed_cap != Some(EXPECTED_MONTHLY_CAP)
    {
        return Err(
            format!("independent verifier did not accept the public state: {receipt}").into(),
        );
    }
    println!(
        "INDEPENDENT_VERIFY valid=true reasons=0 surplus=0 recomputed_cap={EXPECTED_MONTHLY_CAP} \
         receipt_sha256={}",
        receipt["sha256"].as_str().unwrap_or("")
    );

    let closing_balance = ctx.rpc.get_balance(&payer.pubkey())?;
    let spent = opening_balance.saturating_sub(closing_balance);
    if spent > SPEND_CEILING_LAMPORTS {
        return Err(format!("spend {spent} lamports crossed the ceiling").into());
    }

    let result = json!({
        // A loopback rehearsal and a public run produce the same shaped
        // receipt, so the receipt itself must say which one it was.
        "cluster": if is_loopback { "local-rehearsal" } else { "public" },
        "public_cluster": !is_loopback,
        "rpc_url": ctx.rpc_url,
        "genesis_hash": genesis,
        "program_id": program.pubkey().to_string(),
        "programdata": deployment.programdata_address.to_string(),
        "programdata_slot": deployment.programdata_slot,
        "upgrade_authority": authority.pubkey().to_string(),
        "program_created_by_loader": true,
        "programdata_created_by_loader": true,
        "program_signer_present": true,
        "sbf_sha256": deployment.sbf_sha256,
        "sbf_bytes_exact": true,
        "sbf_size_bytes": bytes.len(),
        "program_install": "upgradeable-loader-v3-deploy-with-max-data-len",
        "loader_transactions": {
            "buffer_create": deployment.buffer_create_signature,
            "write_count": deployment.write_transactions,
            "first_write": deployment.first_write_signature,
            "last_write": deployment.last_write_signature,
            "deploy": deployment.deploy_signature,
            "chunk_resends": deployment.chunk_resends,
        },
        "application_transactions": {
            "setup": setup_signature,
            "deposit": deposit_signature,
        },
        "compute_units": { "setup": setup_units, "deposit": deposit_units, "pre_cliff": pre_cliff_units },
        "mint": mint.pubkey().to_string(),
        "depositor": payer.pubkey().to_string(),
        "beneficiary": beneficiary.pubkey().to_string(),
        "depositor_token": depositor_token.pubkey().to_string(),
        "beneficiary_token": beneficiary_token.pubkey().to_string(),
        "vault_state": vault_state.to_string(),
        "vault_token": vault_token.to_string(),
        "deposit_amount": DEPOSIT,
        "monthly_cap": state.monthly_cap,
        "released_total": state.released_total,
        "vault_token_balance": vault_balance,
        "genesis_ts": state.genesis_ts,
        "cliff_end_ts": state.cliff_end_ts,
        "pre_cliff_simulation": "EXPECTED_REJECTION_CLIFF_ACTIVE",
        "release_transaction_sent": false,
        "independent_rpc_verifier": if valid { "PASS" } else { "FAIL" },
        "independent_receipt_sha256": receipt["sha256"],
        "token_surplus_amount": 0,
        "lamports_spent": spent,
        "private_keys_serialized": false,
    });
    println!("RESULT_JSON {result}");
    Ok(())
}
