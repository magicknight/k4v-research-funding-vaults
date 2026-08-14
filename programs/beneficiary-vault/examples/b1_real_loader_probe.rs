// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deploy the exact B1 SBF through the upgradeable loader on an isolated,
//! offline Surfpool process, then execute and independently reconstruct the B1
//! transaction flow.
//!
//! All transaction signers are generated in memory and never serialized. The
//! declared B1 program address is pre-created with Surfpool's local-only
//! account fixture RPC as an uninitialized loader-owned account because its
//! historical keypair was deliberately destroyed. `DeployWithMaxDataLen`
//! still creates ProgramData, verifies the exact SBF bytes, marks the Program
//! executable, and records the in-memory upgrade authority through a real
//! signed loader transaction.

use anchor_lang::{
    solana_program::bpf_loader_upgradeable::{
        self, deploy_with_max_program_len, get_program_data_address, UpgradeableLoaderState,
    },
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use beneficiary_vault::{accounts, instruction, state::BeneficiaryVault};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use solana_clock::Clock;
use solana_commitment_config::CommitmentConfig;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::{config::RpcSimulateTransactionConfig, request::RpcRequest};
use solana_signature::Signature;
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
    io::{self, Write},
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const RPC_PORT: u16 = 18_999;
const DEPOSIT: u64 = 1_000_000_000;
const ANNUAL_BPS: u16 = 500;
const MONTHLY_CAP: u64 = 4_166_666;
const MIN_CLIFF_SECONDS: i64 = 730 * 24 * 60 * 60;
const POLICY_HASH: [u8; 32] = [0x42; 32];
const WRITE_CHUNK_BYTES: usize = 900;
const CLOCK_CROSS_TIMEOUT: Duration = Duration::from_secs(30);

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn program_path() -> PathBuf {
    repository_root().join("target/deploy/beneficiary_vault.so")
}

fn program_fixture_path() -> PathBuf {
    repository_root().join("tests/fixtures/b1_uninitialized_program_account.json")
}

fn rpc_url() -> String {
    format!("http://127.0.0.1:{RPC_PORT}")
}

fn rpc_client() -> RpcClient {
    RpcClient::new_with_commitment(rpc_url(), CommitmentConfig::processed())
}

fn custom_rpc(
    rpc: &RpcClient,
    method: &'static str,
    params: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(rpc.send(RpcRequest::Custom { method }, params)?)
}

fn seed_surfpool_loader_accounts(
    rpc: &RpcClient,
    payer: &Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    let _: Value = custom_rpc(
        rpc,
        "surfnet_setAccount",
        json!([payer.to_string(), {
            "lamports": 500_000_000_000_000_000_u64,
            "owner": system_program::ID.to_string(),
            "executable": false
        }]),
    )?;
    let program_space = UpgradeableLoaderState::size_of_program();
    let program_lamports = rpc.get_minimum_balance_for_rent_exemption(program_space)?;
    let _: Value = custom_rpc(
        rpc,
        "surfnet_setAccount",
        json!([beneficiary_vault::ID.to_string(), {
            "lamports": program_lamports,
            "data": hex::encode(vec![0_u8; program_space]),
            "owner": bpf_loader_upgradeable::ID.to_string(),
            "executable": false,
            "rent_epoch": 0
        }]),
    )?;
    Ok(())
}

fn wait_for_send(stage: &str) -> Result<(), Box<dyn std::error::Error>> {
    if env::var("K4V_LOCAL_REAL_LOADER_SEND_CONFIRMED").as_deref() == Ok("1") {
        println!("LOCAL_REAL_LOADER_SEND_PREAUTHORIZED {stage}");
        return Ok(());
    }
    println!("AWAITING_SEND {stage}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let expected = format!("SEND_{stage}");
    if input.trim() != expected {
        return Err(format!("expected {expected}; transaction was not sent").into());
    }
    Ok(())
}

fn signed_transaction(
    rpc: &RpcClient,
    instructions: &[Instruction],
    payer: &Keypair,
    extra_signers: &[&Keypair],
) -> Result<Transaction, Box<dyn std::error::Error>> {
    let mut signers = Vec::with_capacity(1 + extra_signers.len());
    signers.push(payer);
    signers.extend_from_slice(extra_signers);
    Ok(Transaction::new_signed_with_payer(
        instructions,
        Some(&payer.pubkey()),
        &signers,
        rpc.get_latest_blockhash()?,
    ))
}

fn simulate(
    rpc: &RpcClient,
    tx: &Transaction,
    stage: &str,
    expected_log: Option<&str>,
) -> Result<u64, Box<dyn std::error::Error>> {
    let result = rpc
        .simulate_transaction_with_config(
            tx,
            RpcSimulateTransactionConfig {
                sig_verify: true,
                commitment: Some(CommitmentConfig::processed()),
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
                    "{stage} simulation did not produce expected {needle}: err={:?}; logs={logs:?}",
                    result.err
                )
                .into());
            }
        }
    }
    Ok(units)
}

fn send_simulated(
    rpc: &RpcClient,
    tx: &Transaction,
    stage: &str,
) -> Result<(String, u64), Box<dyn std::error::Error>> {
    let units = simulate(rpc, tx, stage, None)?;
    let signature = rpc.send_and_confirm_transaction(tx)?.to_string();
    Ok((signature, units))
}

fn wait_for_finalized(signature: &Signature) -> Result<(), Box<dyn std::error::Error>> {
    let rpc = RpcClient::new_with_commitment(rpc_url(), CommitmentConfig::finalized());
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if rpc
            .confirm_transaction_with_commitment(signature, CommitmentConfig::finalized())?
            .value
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("transaction {signature} did not reach finalized").into());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn token_amount(rpc: &RpcClient, address: &Pubkey) -> Result<u64, Box<dyn std::error::Error>> {
    let account = rpc.get_account(address)?;
    Ok(SplAccount::unpack(&account.data)?.amount)
}

fn read_state(
    rpc: &RpcClient,
    program_id: &Pubkey,
    address: &Pubkey,
) -> Result<BeneficiaryVault, Box<dyn std::error::Error>> {
    let account = rpc.get_account(address)?;
    if account.owner != *program_id {
        return Err("vault state owner did not match deployed program".into());
    }
    Ok(BeneficiaryVault::try_deserialize(
        &mut account.data.as_slice(),
    )?)
}

fn read_clock(rpc: &RpcClient) -> Result<Clock, Box<dyn std::error::Error>> {
    let account = rpc.get_account(&solana_clock::sysvar::ID)?;
    Ok(bincode::deserialize(&account.data)?)
}

fn wait_until_cliff_crossed(
    rpc: &RpcClient,
    cliff_end_ts: i64,
) -> Result<Clock, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + CLOCK_CROSS_TIMEOUT;
    loop {
        let clock = read_clock(rpc)?;
        if clock.unix_timestamp >= cliff_end_ts {
            return Ok(clock);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "validator clock did not cross cliff after slot warp: now={} cliff={cliff_end_ts}",
                clock.unix_timestamp
            )
            .into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn verify_genesis_program_account(rpc: &RpcClient) -> Result<(), Box<dyn std::error::Error>> {
    let account = rpc.get_account(&beneficiary_vault::ID)?;
    if account.owner != bpf_loader_upgradeable::ID
        || account.executable
        || account.data.len() != UpgradeableLoaderState::size_of_program()
        || account.lamports
            < rpc
                .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program())?
    {
        return Err(
            "genesis Program account did not match the uninitialized loader fixture".into(),
        );
    }
    let state: UpgradeableLoaderState = bincode::deserialize(&account.data)?;
    if state != UpgradeableLoaderState::Uninitialized {
        return Err("genesis Program account was not uninitialized".into());
    }
    Ok(())
}

struct DeploymentReceipt {
    buffer_create_signature: String,
    first_write_signature: String,
    last_write_signature: String,
    deploy_signature: String,
    write_transactions: usize,
    buffer_create_units: u64,
    max_write_units: u64,
    deploy_units: u64,
    programdata_address: Pubkey,
    programdata_slot: u64,
    sbf_sha256: String,
}

fn deploy_exact_program(
    rpc: &RpcClient,
    payer: &Keypair,
    buffer: &Keypair,
    upgrade_authority: &Keypair,
    program_bytes: &[u8],
) -> Result<DeploymentReceipt, Box<dyn std::error::Error>> {
    verify_genesis_program_account(rpc)?;
    let buffer_lamports = rpc.get_minimum_balance_for_rent_exemption(
        UpgradeableLoaderState::size_of_buffer(program_bytes.len()),
    )?;
    let buffer_instructions = bpf_loader_upgradeable::create_buffer(
        &payer.pubkey(),
        &buffer.pubkey(),
        &upgrade_authority.pubkey(),
        buffer_lamports,
        program_bytes.len(),
    )?;
    let buffer_tx = signed_transaction(rpc, &buffer_instructions, payer, &[buffer])?;
    let buffer_create_units = simulate(rpc, &buffer_tx, "LOADER_CREATE_BUFFER", None)?;
    wait_for_send("LOADER_CREATE_BUFFER")?;
    let buffer_create_signature = rpc.send_and_confirm_transaction(&buffer_tx)?.to_string();

    wait_for_send("LOADER_WRITE_CHUNKS")?;
    let mut first_write_signature = None;
    let mut last_write_signature = None;
    let mut max_write_units = 0;
    let write_transactions = program_bytes.len().div_ceil(WRITE_CHUNK_BYTES);
    for (index, chunk) in program_bytes.chunks(WRITE_CHUNK_BYTES).enumerate() {
        let offset = index
            .checked_mul(WRITE_CHUNK_BYTES)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or("loader write offset overflow")?;
        let instruction = bpf_loader_upgradeable::write(
            &buffer.pubkey(),
            &upgrade_authority.pubkey(),
            offset,
            chunk.to_vec(),
        );
        let tx = signed_transaction(rpc, &[instruction], payer, &[upgrade_authority])?;
        let (signature, units) = send_simulated(rpc, &tx, "LOADER_WRITE")?;
        max_write_units = max_write_units.max(units);
        first_write_signature.get_or_insert_with(|| signature.clone());
        last_write_signature = Some(signature);
        if (index + 1) % 32 == 0 || index + 1 == write_transactions {
            println!(
                "LOADER_WRITE_PROGRESS completed={}/{} max_units={max_write_units}",
                index + 1,
                write_transactions
            );
        }
    }

    let program_lamports =
        rpc.get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program())?;
    let deploy_instructions = deploy_with_max_program_len(
        &payer.pubkey(),
        &beneficiary_vault::ID,
        &buffer.pubkey(),
        &upgrade_authority.pubkey(),
        program_lamports,
        program_bytes.len(),
    )?;
    if deploy_instructions.len() != 2 {
        return Err("unexpected loader deploy instruction count".into());
    }
    let deploy_tx = signed_transaction(
        rpc,
        std::slice::from_ref(&deploy_instructions[1]),
        payer,
        &[upgrade_authority],
    )?;
    let deploy_units = simulate(rpc, &deploy_tx, "LOADER_DEPLOY", None)?;
    wait_for_send("LOADER_DEPLOY")?;
    let deploy_signature = rpc.send_and_confirm_transaction(&deploy_tx)?.to_string();

    let program_account = rpc.get_account(&beneficiary_vault::ID)?;
    if program_account.owner != bpf_loader_upgradeable::ID || !program_account.executable {
        return Err("real loader did not expose the B1 Program account as executable".into());
    }
    let program_state: UpgradeableLoaderState = bincode::deserialize(&program_account.data)?;
    let programdata_address = match program_state {
        UpgradeableLoaderState::Program {
            programdata_address,
        } => programdata_address,
        other => return Err(format!("unexpected Program state: {other:?}").into()),
    };
    if programdata_address != get_program_data_address(&beneficiary_vault::ID) {
        return Err("ProgramData PDA did not reconcile".into());
    }
    let programdata = rpc.get_account(&programdata_address)?;
    if programdata.owner != bpf_loader_upgradeable::ID || programdata.executable {
        return Err("ProgramData owner or executable flag was invalid".into());
    }
    let metadata_len = UpgradeableLoaderState::size_of_programdata_metadata();
    if programdata.data.len() != metadata_len + program_bytes.len()
        || &programdata.data[metadata_len..] != program_bytes
    {
        return Err("ProgramData bytes did not equal the exact B1 SBF".into());
    }
    let programdata_state: UpgradeableLoaderState =
        bincode::deserialize(&programdata.data[..metadata_len])?;
    let programdata_slot = match programdata_state {
        UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address,
        } if upgrade_authority_address == Some(upgrade_authority.pubkey()) => slot,
        other => return Err(format!("unexpected ProgramData state: {other:?}").into()),
    };
    let sbf_sha256 = hex::encode(Sha256::digest(program_bytes));
    println!(
        "REAL_LOADER_DEPLOYED program={} programdata={} slot={} sbf_sha256={} write_transactions={}",
        beneficiary_vault::ID,
        programdata_address,
        programdata_slot,
        sbf_sha256,
        write_transactions
    );
    Ok(DeploymentReceipt {
        buffer_create_signature,
        first_write_signature: first_write_signature.ok_or("no loader write transaction")?,
        last_write_signature: last_write_signature.ok_or("no loader write transaction")?,
        deploy_signature,
        write_transactions,
        buffer_create_units,
        max_write_units,
        deploy_units,
        programdata_address,
        programdata_slot,
        sbf_sha256,
    })
}

fn run_python_rpc_verifier(
    program_id: &Pubkey,
    vault_state: &Pubkey,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let output = Command::new("python3")
        .current_dir(repository_root())
        .env("PYTHONPATH", "src")
        .arg("src/beneficiary_vault_rpc_exporter.py")
        .arg("--rpc-url")
        .arg(rpc_url())
        .arg("--program-id")
        .arg(program_id.to_string())
        .arg("--vault-state")
        .arg(vault_state.to_string())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "independent Python RPC verifier failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !program_path().is_file() || !program_fixture_path().is_file() {
        return Err("build the B1 SBF and retain the public Program fixture first".into());
    }
    if env::var("K4V_SURFPOOL_REAL_LOADER").as_deref() != Ok("1") {
        return Err("set K4V_SURFPOOL_REAL_LOADER=1 for the loopback-only Surfpool probe".into());
    }
    let payer = Keypair::new();
    let beneficiary = Keypair::new();
    let mint = Keypair::new();
    let depositor_token = Keypair::new();
    let beneficiary_token = Keypair::new();
    let buffer = Keypair::new();
    let upgrade_authority = Keypair::new();
    let rpc = rpc_client();
    if rpc.get_health().is_err() {
        return Err("loopback Surfpool RPC is not healthy".into());
    }
    seed_surfpool_loader_accounts(&rpc, &payer.pubkey())?;
    let version = rpc.get_version()?;
    let payer_balance = rpc.get_balance(&payer.pubkey())?;
    if payer_balance < 20_000_000_000 {
        return Err("genesis-funded ephemeral payer balance was insufficient".into());
    }
    println!(
        "CLUSTER local-surfpool rpc={} version={version:?} payer_lamports={payer_balance}",
        rpc_url()
    );

    let program_bytes = fs::read(program_path())?;
    let deployment =
        deploy_exact_program(&rpc, &payer, &buffer, &upgrade_authority, &program_bytes)?;

    let mint_rent = rpc.get_minimum_balance_for_rent_exemption(Mint::LEN)?;
    let token_rent = rpc.get_minimum_balance_for_rent_exemption(SplAccount::LEN)?;
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
    let setup_tx = signed_transaction(
        &rpc,
        &setup_instructions,
        &payer,
        &[&mint, &depositor_token, &beneficiary_token],
    )?;
    let setup_units = simulate(&rpc, &setup_tx, "SETUP", None)?;
    wait_for_send("SETUP")?;
    let setup_signature = rpc.send_and_confirm_transaction(&setup_tx)?.to_string();

    let (vault_state, _) = Pubkey::find_program_address(
        &[
            b"beneficiary-vault",
            beneficiary.pubkey().as_ref(),
            mint.pubkey().as_ref(),
            POLICY_HASH.as_ref(),
        ],
        &beneficiary_vault::ID,
    );
    let (vault_token, _) = Pubkey::find_program_address(
        &[b"beneficiary-token", vault_state.as_ref()],
        &beneficiary_vault::ID,
    );
    let deposit_ix = Instruction {
        program_id: beneficiary_vault::ID,
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
            cliff_seconds: MIN_CLIFF_SECONDS,
            policy_hash: POLICY_HASH,
        }
        .data(),
    };
    let deposit_tx = signed_transaction(&rpc, &[deposit_ix], &payer, &[])?;
    let deposit_units = simulate(&rpc, &deposit_tx, "DEPOSIT", None)?;
    wait_for_send("DEPOSIT")?;
    let deposit_signature_value = rpc.send_and_confirm_transaction(&deposit_tx)?;
    wait_for_finalized(&deposit_signature_value)?;
    let deposit_signature = deposit_signature_value.to_string();
    let deposited_state = read_state(&rpc, &beneficiary_vault::ID, &vault_state)?;
    if deposited_state.deposited_amount != DEPOSIT
        || deposited_state.monthly_cap != MONTHLY_CAP
        || token_amount(&rpc, &vault_token)? != DEPOSIT
        || token_amount(&rpc, &depositor_token.pubkey())? != 0
    {
        return Err("deposit state did not reconcile".into());
    }

    let release_ix = Instruction {
        program_id: beneficiary_vault::ID,
        accounts: accounts::Release {
            beneficiary: beneficiary.pubkey(),
            mint: mint.pubkey(),
            vault_state,
            vault_token,
            beneficiary_token: beneficiary_token.pubkey(),
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::Release {
            amount: MONTHLY_CAP,
        }
        .data(),
    };
    let pre_cliff_tx = signed_transaction(
        &rpc,
        std::slice::from_ref(&release_ix),
        &payer,
        &[&beneficiary],
    )?;
    let pre_cliff_units = simulate(
        &rpc,
        &pre_cliff_tx,
        "PRE_CLIFF_RELEASE",
        Some("CliffActive"),
    )?;

    let _: Value = custom_rpc(
        &rpc,
        "surfnet_timeTravel",
        json!([{"absoluteTimestamp": deposited_state.cliff_end_ts * 1_000}]),
    )?;
    let warped_clock = wait_until_cliff_crossed(&rpc, deposited_state.cliff_end_ts)?;
    let warp_slot = rpc.get_slot()?;
    let persisted_program = rpc.get_account(&beneficiary_vault::ID)?;
    if !persisted_program.executable || persisted_program.owner != bpf_loader_upgradeable::ID {
        return Err("real-loader Program account did not survive Surfpool time travel".into());
    }

    let release_tx = signed_transaction(&rpc, &[release_ix], &payer, &[&beneficiary])?;
    let release_units = simulate(&rpc, &release_tx, "RELEASE", None)?;
    wait_for_send("RELEASE")?;
    let release_signature_value = rpc.send_and_confirm_transaction(&release_tx)?;
    wait_for_finalized(&release_signature_value)?;
    let release_signature = release_signature_value.to_string();
    let released_state = read_state(&rpc, &beneficiary_vault::ID, &vault_state)?;
    let vault_balance = token_amount(&rpc, &vault_token)?;
    let beneficiary_balance = token_amount(&rpc, &beneficiary_token.pubkey())?;
    if released_state.released_total != MONTHLY_CAP
        || released_state.released_this_period != MONTHLY_CAP
        || vault_balance != DEPOSIT - MONTHLY_CAP
        || beneficiary_balance != MONTHLY_CAP
    {
        return Err("release state did not reconcile".into());
    }

    let rpc_snapshot = run_python_rpc_verifier(&beneficiary_vault::ID, &vault_state)?;
    if rpc_snapshot["program_id"] != beneficiary_vault::ID.to_string()
        || rpc_snapshot["vault_state"] != vault_state.to_string()
        || rpc_snapshot["token_account"]["amount"] != vault_balance
    {
        return Err("independent Python RPC snapshot did not reconcile".into());
    }
    println!(
        "INDEPENDENT_RPC_VERIFIER PASS observed_at_ts={}",
        rpc_snapshot["observed_at_ts"]
    );

    println!(
        "RESULT_JSON {}",
        serde_json::to_string(&json!({
            "schema": "k4v-b1-real-loader-probe/v0.1",
            "cluster": "local-surfpool",
            "rpc_url": rpc_url(),
            "program_id": beneficiary_vault::ID.to_string(),
            "program_install": "upgradeable-loader-v3-deploy-with-max-data-len",
            "program_account_origin": "surfnet-set-account-uninitialized-loader-owned-fixture",
            "program_signer_present": false,
            "programdata": deployment.programdata_address.to_string(),
            "programdata_slot": deployment.programdata_slot,
            "upgrade_authority": upgrade_authority.pubkey().to_string(),
            "sbf_sha256": deployment.sbf_sha256,
            "sbf_bytes_exact": true,
            "ephemeral_signers_only": true,
            "private_keys_serialized": false,
            "surfpool_storage_expected": "ephemeral-memory",
            "classic_spl_token": true,
            "mint": mint.pubkey().to_string(),
            "depositor": payer.pubkey().to_string(),
            "beneficiary": beneficiary.pubkey().to_string(),
            "depositor_token": depositor_token.pubkey().to_string(),
            "beneficiary_token": beneficiary_token.pubkey().to_string(),
            "vault_state": vault_state.to_string(),
            "vault_token": vault_token.to_string(),
            "deposit_amount": DEPOSIT,
            "monthly_cap": MONTHLY_CAP,
            "released_total": released_state.released_total,
            "vault_token_balance": vault_balance,
            "beneficiary_token_balance": beneficiary_balance,
            "pre_cliff_simulation": "EXPECTED_REJECTION_CLIFF_ACTIVE",
            "warped_slot": warp_slot,
            "warped_unix_timestamp": warped_clock.unix_timestamp,
            "independent_rpc_verifier": "PASS",
            "simulations": {
                "loader_create_buffer_units": deployment.buffer_create_units,
                "loader_max_write_units": deployment.max_write_units,
                "loader_deploy_units": deployment.deploy_units,
                "setup_units": setup_units,
                "deposit_units": deposit_units,
                "pre_cliff_units": pre_cliff_units,
                "release_units": release_units
            },
            "loader_transactions": {
                "buffer_create": deployment.buffer_create_signature,
                "write_count": deployment.write_transactions,
                "first_write": deployment.first_write_signature,
                "last_write": deployment.last_write_signature,
                "deploy": deployment.deploy_signature
            },
            "application_transactions": {
                "setup": setup_signature,
                "deposit": deposit_signature,
                "release": release_signature
            }
        }))?
    );
    Ok(())
}
