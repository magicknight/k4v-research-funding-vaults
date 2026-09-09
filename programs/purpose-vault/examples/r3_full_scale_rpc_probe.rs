// SPDX-License-Identifier: MIT OR Apache-2.0
//! R3-B full-scale local Surfpool transaction and raw-RPC probe.
//!
//! The program SBF is loaded through Surfpool's local-only cheatcode. Every
//! mint and token account is then created by signed System/SPL transactions;
//! no token account or mint data is injected. All keypairs remain in memory.

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use purpose_vault::{
    accounts,
    constants::{
        APPROVAL_SEED, MARKET_SEED, PERIOD_SECONDS, POLICY_SEED, TOKEN_VAULT_SEED, VAULT_SEED,
    },
    instruction,
    state::{Approval, CovenantVault, MarketInput, PolicyWindow, VaultKind},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use solana_commitment_config::CommitmentConfig;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_program_option::COption;
use solana_program_pack::Pack;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::{config::RpcSimulateTransactionConfig, request::RpcRequest};
use solana_signer::Signer;
use solana_system_interface::{instruction as system_instruction, program as system_program};
use solana_transaction::Transaction;
use spl_token_interface::{
    instruction::{self as token_instruction, AuthorityType},
    state::{Account as SplAccount, Mint},
    ID as TOKEN_PROGRAM_ID,
};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const DECIMALS: u8 = 9;
const TOTAL_SUPPLY: u64 = 1_000_000_000_000_000_000;
const FOUNDER: u64 = 300_000_000_000_000_000;
const TREASURY: u64 = 500_000_000_000_000_000;
const GENESIS: u64 = 120_000_000_000_000_000;
const LP: u64 = 80_000_000_000_000_000;
const ANNUAL_BPS: u16 = 500;
const FOUNDER_CAP: u64 = 1_250_000_000_000_000;
const TREASURY_CAP: u64 = 2_083_333_333_333_333;
const MARKET_BPS: u16 = 250;
const ELIGIBLE_VOLUME: u64 = 120_000_000_000_000_000;
const MARKET_CAPACITY: u64 = 3_000_000_000_000_000;
const MAX_AGE_SECONDS: i64 = 3 * 24 * 60 * 60;
const MIN_CLIFF_SECONDS: i64 = 730 * 24 * 60 * 60;
const POLICY_SPEC_HASH: [u8; 32] = [0x73; 32];
const LOCAL_LAMPORTS: u64 = 20_000_000_000;

const _: () = assert!(FOUNDER + TREASURY + GENESIS + LP == TOTAL_SUPPLY);
const _: () = assert!(FOUNDER_CAP + TREASURY_CAP > MARKET_CAPACITY);

fn program_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy/purpose_vault.so")
}

fn candidate_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env::var("K4V_R3_CANDIDATE_CONFIG").map_err(
        |_| "K4V_R3_CANDIDATE_CONFIG must name the explicit open candidate config",
    )?))
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(hex::encode(Sha256::digest(fs::read(path)?)))
}

fn wait_for_send(stage: &str) -> Result<(), Box<dyn std::error::Error>> {
    if env::var("K4V_LOCAL_TRANSACTION_SEND_CONFIRMED").as_deref() == Ok("1") {
        println!("LOCAL_SEND_PREAUTHORIZED {stage}");
        return Ok(());
    }
    println!("AWAITING_LOCAL_SEND {stage}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let expected = format!("SEND_{stage}");
    if input.trim() != expected {
        return Err(format!("expected {expected}; transaction was not sent").into());
    }
    Ok(())
}

fn custom_rpc(
    rpc: &RpcClient,
    method: &'static str,
    params: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(rpc.send(RpcRequest::Custom { method }, params)?)
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
) -> Result<(), Box<dyn std::error::Error>> {
    let result = rpc
        .simulate_transaction_with_config(
            tx,
            RpcSimulateTransactionConfig {
                sig_verify: true,
                ..RpcSimulateTransactionConfig::default()
            },
        )?
        .value;
    let logs = result.logs.unwrap_or_default();
    match expected_log {
        None => {
            if let Some(error) = result.err {
                return Err(format!("{stage} simulation failed: {error:?}; logs={logs:?}").into());
            }
            println!(
                "SIMULATION {stage} PASS units_consumed={}",
                result.units_consumed.unwrap_or_default()
            );
        }
        Some(needle) => {
            if result.err.is_none() || !logs.iter().any(|line| line.contains(needle)) {
                return Err(format!(
                    "{stage} simulation did not produce {needle}: err={:?}; logs={logs:?}",
                    result.err
                )
                .into());
            }
            println!("SIMULATION {stage} EXPECTED_REJECTION {needle}");
        }
    }
    Ok(())
}

fn send_local(
    rpc: &RpcClient,
    tx: &Transaction,
    stage: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    simulate(rpc, tx, stage, None)?;
    wait_for_send(stage)?;
    let signature = rpc.send_and_confirm_transaction(tx)?;
    println!("TRANSACTION {stage} CONFIRMED signature={signature}");
    Ok(signature.to_string())
}

fn fund_local_account(rpc: &RpcClient, key: Pubkey) -> Result<(), Box<dyn std::error::Error>> {
    let _: Value = custom_rpc(
        rpc,
        "surfnet_setAccount",
        json!([key.to_string(), {
            "lamports": LOCAL_LAMPORTS,
            "owner": system_program::ID.to_string(),
            "executable": false
        }]),
    )?;
    if rpc.get_balance(&key)? != LOCAL_LAMPORTS {
        return Err(format!("local funding failed for {key}").into());
    }
    Ok(())
}

fn create_token_account(
    rpc: &RpcClient,
    payer: &Keypair,
    mint: Pubkey,
    owner: Pubkey,
    label: &str,
) -> Result<(Keypair, String), Box<dyn std::error::Error>> {
    let account = Keypair::new();
    let rent = rpc.get_minimum_balance_for_rent_exemption(SplAccount::LEN)?;
    let tx = signed_transaction(
        rpc,
        &[
            system_instruction::create_account(
                &payer.pubkey(),
                &account.pubkey(),
                rent,
                SplAccount::LEN as u64,
                &TOKEN_PROGRAM_ID,
            ),
            token_instruction::initialize_account3(
                &TOKEN_PROGRAM_ID,
                &account.pubkey(),
                &mint,
                &owner,
            )?,
        ],
        payer,
        &[&account],
    )?;
    let signature = send_local(rpc, &tx, label)?;
    Ok((account, signature))
}

fn raw_mint(rpc: &RpcClient, address: Pubkey) -> Result<Mint, Box<dyn std::error::Error>> {
    let account = rpc.get_account(&address)?;
    if account.owner != TOKEN_PROGRAM_ID || account.data.len() != Mint::LEN {
        return Err("mint owner or data length mismatch".into());
    }
    Ok(Mint::unpack(&account.data)?)
}

fn raw_token(
    rpc: &RpcClient,
    address: Pubkey,
    expected_mint: Pubkey,
) -> Result<SplAccount, Box<dyn std::error::Error>> {
    let account = rpc.get_account(&address)?;
    if account.owner != TOKEN_PROGRAM_ID || account.data.len() != SplAccount::LEN {
        return Err(format!("token account {address} owner or data length mismatch").into());
    }
    let token = SplAccount::unpack(&account.data)?;
    if token.mint != expected_mint {
        return Err(format!("token account {address} mint mismatch").into());
    }
    Ok(token)
}

fn raw_anchor<T: AccountDeserialize>(
    rpc: &RpcClient,
    address: Pubkey,
) -> Result<T, Box<dyn std::error::Error>> {
    let account = rpc.get_account(&address)?;
    if account.owner != purpose_vault::ID {
        return Err(format!("B2 account {address} owner mismatch").into());
    }
    Ok(T::try_deserialize(&mut account.data.as_slice())?)
}

fn policy_pda(policy_hash: [u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[POLICY_SEED, policy_hash.as_ref()], &purpose_vault::ID).0
}

fn market_pda(policy_hash: [u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[MARKET_SEED, policy_hash.as_ref()], &purpose_vault::ID).0
}

fn vault_pda(
    kind: VaultKind,
    authority: Pubkey,
    mint: Pubkey,
    policy_hash: [u8; 32],
) -> (Pubkey, Pubkey) {
    let vault = Pubkey::find_program_address(
        &[
            VAULT_SEED,
            policy_hash.as_ref(),
            &[kind.seed_byte()],
            authority.as_ref(),
            mint.as_ref(),
        ],
        &purpose_vault::ID,
    )
    .0;
    let token =
        Pubkey::find_program_address(&[TOKEN_VAULT_SEED, vault.as_ref()], &purpose_vault::ID).0;
    (vault, token)
}

fn approval_pda(vault: Pubkey, period_index: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[APPROVAL_SEED, vault.as_ref(), &period_index.to_le_bytes()],
        &purpose_vault::ID,
    )
    .0
}

fn count_open_parameters(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            usize::from(map.get("state").and_then(Value::as_str) == Some("OPEN"))
                + map.values().map(count_open_parameters).sum::<usize>()
        }
        Value::Array(values) => values.iter().map(count_open_parameters).sum(),
        _ => 0,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rpc_url =
        env::var("K4V_SURFPOOL_RPC").unwrap_or_else(|_| "http://127.0.0.1:18999".to_string());
    if !rpc_url.starts_with("http://127.0.0.1:") && !rpc_url.starts_with("http://localhost:") {
        return Err("R3-B probe is restricted to a loopback RPC URL".into());
    }
    let rpc = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::confirmed());
    let version = rpc.get_version()?;
    let genesis_hash = rpc.get_genesis_hash()?.to_string();
    println!("CLUSTER local-surfpool rpc={rpc_url} version={version:?}");

    let candidate_path = candidate_path()?;
    let candidate: Value = serde_json::from_slice(&fs::read(&candidate_path)?)?;
    let candidate_sha256 = sha256_file(&candidate_path)?;
    let open_parameter_count = candidate
        .get("open_parameter_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or_else(|| count_open_parameters(&candidate));
    if candidate.get("mainnet_authorized") != Some(&Value::Bool(false))
        || !candidate.get("official_mint").is_some_and(Value::is_null)
        || open_parameter_count == 0
    {
        return Err("candidate config lost its explicit no-mainnet/open boundary".into());
    }

    let sbf = fs::read(program_path())?;
    let sbf_sha256 = hex::encode(Sha256::digest(&sbf));
    let _: Value = custom_rpc(
        &rpc,
        "surfnet_writeProgram",
        json!([purpose_vault::ID.to_string(), hex::encode(&sbf), 0]),
    )?;
    let loaded = rpc.get_account(&purpose_vault::ID)?;
    if !loaded.executable {
        return Err("Surfpool did not expose the loaded B2 program as executable".into());
    }

    let payer = Keypair::new();
    let policy_authority = Keypair::new();
    let oracle = Keypair::new();
    let depositor = Keypair::new();
    let beneficiary = Keypair::new();
    let approver = Keypair::new();
    let contractor = Keypair::new();
    for key in [&payer, &policy_authority, &depositor, &approver] {
        fund_local_account(&rpc, key.pubkey())?;
    }

    let mint = Keypair::new();
    let mint_rent = rpc.get_minimum_balance_for_rent_exemption(Mint::LEN)?;
    let setup_mint = signed_transaction(
        &rpc,
        &[
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
                Some(&payer.pubkey()),
                DECIMALS,
            )?,
        ],
        &payer,
        &[&mint],
    )?;
    let mut signatures = serde_json::Map::new();
    signatures.insert(
        "create_mint".into(),
        send_local(&rpc, &setup_mint, "CREATE_MINT")?.into(),
    );

    let (founder_token, sig) = create_token_account(
        &rpc,
        &payer,
        mint.pubkey(),
        depositor.pubkey(),
        "CREATE_FOUNDER_ACCOUNT",
    )?;
    signatures.insert("create_founder_account".into(), sig.into());
    let (treasury_token, sig) = create_token_account(
        &rpc,
        &payer,
        mint.pubkey(),
        depositor.pubkey(),
        "CREATE_TREASURY_ACCOUNT",
    )?;
    signatures.insert("create_treasury_account".into(), sig.into());
    let (genesis_token, sig) = create_token_account(
        &rpc,
        &payer,
        mint.pubkey(),
        Pubkey::new_unique(),
        "CREATE_GENESIS_ACCOUNT",
    )?;
    signatures.insert("create_genesis_account".into(), sig.into());
    let (lp_token, sig) = create_token_account(
        &rpc,
        &payer,
        mint.pubkey(),
        Pubkey::new_unique(),
        "CREATE_LP_ACCOUNT",
    )?;
    signatures.insert("create_lp_account".into(), sig.into());
    let (beneficiary_token, sig) = create_token_account(
        &rpc,
        &payer,
        mint.pubkey(),
        beneficiary.pubkey(),
        "CREATE_BENEFICIARY_DESTINATION",
    )?;
    signatures.insert("create_beneficiary_destination".into(), sig.into());
    let (contractor_token, sig) = create_token_account(
        &rpc,
        &payer,
        mint.pubkey(),
        contractor.pubkey(),
        "CREATE_CONTRACTOR_DESTINATION",
    )?;
    signatures.insert("create_contractor_destination".into(), sig.into());

    let mint_allocations = [
        (founder_token.pubkey(), FOUNDER),
        (treasury_token.pubkey(), TREASURY),
        (genesis_token.pubkey(), GENESIS),
        (lp_token.pubkey(), LP),
    ]
    .into_iter()
    .map(|(account, amount)| {
        token_instruction::mint_to(
            &TOKEN_PROGRAM_ID,
            &mint.pubkey(),
            &account,
            &payer.pubkey(),
            &[],
            amount,
        )
    })
    .collect::<Result<Vec<_>, _>>()?;
    let tx = signed_transaction(&rpc, &mint_allocations, &payer, &[])?;
    signatures.insert(
        "mint_four_allocations".into(),
        send_local(&rpc, &tx, "MINT_FOUR_ALLOCATIONS")?.into(),
    );

    let revoke = signed_transaction(
        &rpc,
        &[
            token_instruction::set_authority(
                &TOKEN_PROGRAM_ID,
                &mint.pubkey(),
                None,
                AuthorityType::MintTokens,
                &payer.pubkey(),
                &[],
            )?,
            token_instruction::set_authority(
                &TOKEN_PROGRAM_ID,
                &mint.pubkey(),
                None,
                AuthorityType::FreezeAccount,
                &payer.pubkey(),
                &[],
            )?,
        ],
        &payer,
        &[],
    )?;
    signatures.insert(
        "revoke_mint_freeze".into(),
        send_local(&rpc, &revoke, "REVOKE_MINT_FREEZE")?.into(),
    );

    let mint_one_more = signed_transaction(
        &rpc,
        &[token_instruction::mint_to(
            &TOKEN_PROGRAM_ID,
            &mint.pubkey(),
            &beneficiary_token.pubkey(),
            &payer.pubkey(),
            &[],
            1,
        )?],
        &payer,
        &[],
    )?;
    simulate(
        &rpc,
        &mint_one_more,
        "R3_N01_MINT_AFTER_REVOCATION",
        Some("the total supply of this token is fixed"),
    )?;

    let policy_hash = purpose_vault::instructions::open_policy::bound_policy_hash(
        &policy_authority.pubkey(),
        &mint.pubkey(),
        &POLICY_SPEC_HASH,
    );
    let policy = policy_pda(policy_hash);
    let market = market_pda(policy_hash);
    let open_policy = Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::OpenPolicy {
            authority: policy_authority.pubkey(),
            oracle: oracle.pubkey(),
            mint: mint.pubkey(),
            policy,
            market,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: instruction::OpenPolicy {
            policy_spec_hash: POLICY_SPEC_HASH,
            market_capacity_bps: MARKET_BPS,
            max_age_seconds: MAX_AGE_SECONDS,
            hard_ceiling: u64::MAX,
            silence_floor: 0,
            silence_grace_seconds: 0,
        }
        .data(),
    };
    let tx = signed_transaction(&rpc, &[open_policy], &payer, &[&policy_authority])?;
    signatures.insert(
        "open_policy".into(),
        send_local(&rpc, &tx, "OPEN_POLICY")?.into(),
    );

    let (beneficiary_vault, beneficiary_vault_token) = vault_pda(
        VaultKind::Beneficiary,
        beneficiary.pubkey(),
        mint.pubkey(),
        policy_hash,
    );
    let founder_deposit = Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::Deposit {
            depositor: depositor.pubkey(),
            policy_authority: policy_authority.pubkey(),
            authority: beneficiary.pubkey(),
            mint: mint.pubkey(),
            depositor_token: founder_token.pubkey(),
            policy,
            vault: beneficiary_vault,
            vault_token: beneficiary_vault_token,
            token_program: TOKEN_PROGRAM_ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: instruction::Deposit {
            kind: VaultKind::Beneficiary,
            amount: FOUNDER,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
        }
        .data(),
    };
    let tx = signed_transaction(
        &rpc,
        &[founder_deposit],
        &payer,
        &[&depositor, &policy_authority],
    )?;
    signatures.insert(
        "deposit_founder".into(),
        send_local(&rpc, &tx, "DEPOSIT_FOUNDER")?.into(),
    );

    let (purpose_vault, purpose_vault_token) = vault_pda(
        VaultKind::Purpose,
        approver.pubkey(),
        mint.pubkey(),
        policy_hash,
    );
    let treasury_deposit = Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::Deposit {
            depositor: depositor.pubkey(),
            policy_authority: policy_authority.pubkey(),
            authority: approver.pubkey(),
            mint: mint.pubkey(),
            depositor_token: treasury_token.pubkey(),
            policy,
            vault: purpose_vault,
            vault_token: purpose_vault_token,
            token_program: TOKEN_PROGRAM_ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: instruction::Deposit {
            kind: VaultKind::Purpose,
            amount: TREASURY,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: 0,
        }
        .data(),
    };
    let tx = signed_transaction(
        &rpc,
        &[treasury_deposit],
        &payer,
        &[&depositor, &policy_authority],
    )?;
    signatures.insert(
        "deposit_treasury".into(),
        send_local(&rpc, &tx, "DEPOSIT_TREASURY")?.into(),
    );

    let beneficiary_state: CovenantVault = raw_anchor(&rpc, beneficiary_vault)?;
    let pre_cliff = Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ReleaseBeneficiary {
            beneficiary: beneficiary.pubkey(),
            mint: mint.pubkey(),
            vault: beneficiary_vault,
            policy,
            market,
            vault_token: beneficiary_vault_token,
            destination: beneficiary_token.pubkey(),
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::ReleaseBeneficiary { amount: 1 }.data(),
    };
    let tx = signed_transaction(&rpc, &[pre_cliff], &payer, &[&beneficiary])?;
    simulate(&rpc, &tx, "R3_N03_PRE_CLIFF", Some("CliffActive"))?;

    let period_index = MIN_CLIFF_SECONDS.div_euclid(PERIOD_SECONDS) as u64 + 2;
    let approval = approval_pda(purpose_vault, period_index);
    let approve = Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::Approve {
            approver: approver.pubkey(),
            mint: mint.pubkey(),
            vault: purpose_vault,
            policy,
            destination: contractor_token.pubkey(),
            approval,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: instruction::Approve {
            period_index,
            approved_need: TREASURY_CAP,
        }
        .data(),
    };
    let tx = signed_transaction(&rpc, &[approve], &payer, &[&approver])?;
    signatures.insert(
        "approve_purpose".into(),
        send_local(&rpc, &tx, "APPROVE_PURPOSE")?.into(),
    );

    let target_ts = beneficiary_state.genesis_ts + PERIOD_SECONDS * period_index as i64;
    let _: Value = custom_rpc(
        &rpc,
        "surfnet_timeTravel",
        json!([{"absoluteTimestamp": target_ts * 1_000}]),
    )?;
    let report = Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ReportVolume {
            oracle: oracle.pubkey(),
            market,
        }
        .to_account_metas(None),
        data: instruction::ReportVolume {
            eligible_volume: ELIGIBLE_VOLUME,
        }
        .data(),
    };
    let tx = signed_transaction(&rpc, &[report], &payer, &[&oracle])?;
    signatures.insert(
        "report_volume".into(),
        send_local(&rpc, &tx, "REPORT_VOLUME")?.into(),
    );

    let release_founder = Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ReleaseBeneficiary {
            beneficiary: beneficiary.pubkey(),
            mint: mint.pubkey(),
            vault: beneficiary_vault,
            policy,
            market,
            vault_token: beneficiary_vault_token,
            destination: beneficiary_token.pubkey(),
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::ReleaseBeneficiary {
            amount: FOUNDER_CAP,
        }
        .data(),
    };
    let tx = signed_transaction(&rpc, &[release_founder], &payer, &[&beneficiary])?;
    signatures.insert(
        "release_founder".into(),
        send_local(&rpc, &tx, "RELEASE_FOUNDER")?.into(),
    );

    let purpose_release = |amount| Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ReleasePurpose {
            approver: approver.pubkey(),
            mint: mint.pubkey(),
            vault: purpose_vault,
            policy,
            market,
            vault_token: purpose_vault_token,
            destination: contractor_token.pubkey(),
            approval,
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::ReleasePurpose { amount }.data(),
    };
    let headroom = MARKET_CAPACITY - FOUNDER_CAP;
    let tx = signed_transaction(&rpc, &[purpose_release(headroom + 1)], &payer, &[&approver])?;
    simulate(
        &rpc,
        &tx,
        "R3_N06_SHARED_CAP_PLUS_ONE",
        Some("AggregateCapacityExceeded"),
    )?;
    let tx = signed_transaction(&rpc, &[purpose_release(headroom)], &payer, &[&approver])?;
    signatures.insert(
        "release_purpose".into(),
        send_local(&rpc, &tx, "RELEASE_PURPOSE")?.into(),
    );

    let mint_state = raw_mint(&rpc, mint.pubkey())?;
    let founder_staging = raw_token(&rpc, founder_token.pubkey(), mint.pubkey())?;
    let treasury_staging = raw_token(&rpc, treasury_token.pubkey(), mint.pubkey())?;
    let genesis_state = raw_token(&rpc, genesis_token.pubkey(), mint.pubkey())?;
    let lp_state = raw_token(&rpc, lp_token.pubkey(), mint.pubkey())?;
    let beneficiary_vault_tokens = raw_token(&rpc, beneficiary_vault_token, mint.pubkey())?;
    let purpose_vault_tokens = raw_token(&rpc, purpose_vault_token, mint.pubkey())?;
    let beneficiary_destination = raw_token(&rpc, beneficiary_token.pubkey(), mint.pubkey())?;
    let contractor_destination = raw_token(&rpc, contractor_token.pubkey(), mint.pubkey())?;
    let policy_state: PolicyWindow = raw_anchor(&rpc, policy)?;
    let market_state: MarketInput = raw_anchor(&rpc, market)?;
    let beneficiary_state: CovenantVault = raw_anchor(&rpc, beneficiary_vault)?;
    let purpose_state: CovenantVault = raw_anchor(&rpc, purpose_vault)?;
    let approval_state: Approval = raw_anchor(&rpc, approval)?;

    let conserved = founder_staging.amount
        + treasury_staging.amount
        + genesis_state.amount
        + lp_state.amount
        + beneficiary_vault_tokens.amount
        + purpose_vault_tokens.amount
        + beneficiary_destination.amount
        + contractor_destination.amount;
    let valid = mint_state.supply == TOTAL_SUPPLY
        && mint_state.mint_authority == COption::None
        && mint_state.freeze_authority == COption::None
        && founder_staging.amount == 0
        && treasury_staging.amount == 0
        && beneficiary_state.deposited_amount == FOUNDER
        && purpose_state.deposited_amount == TREASURY
        && beneficiary_state.monthly_cap == FOUNDER_CAP
        && purpose_state.monthly_cap == TREASURY_CAP
        && policy_state.released_this_period == MARKET_CAPACITY
        && market_state.eligible_volume == ELIGIBLE_VOLUME
        && approval_state.consumed == headroom
        && conserved == mint_state.supply;
    if !valid {
        return Err("raw-RPC reconstruction failed an R3-B invariant".into());
    }

    let receipt = json!({
        "schema": "k4v-r3-b-full-scale-local-rpc/v0.1",
        "epistemic_status": "ESTABLISHED_LOCAL_TEST_EVIDENCE_NOT_INDEPENDENT",
        "valid": true,
        "cluster": "local-surfpool",
        "rpc_url": rpc_url,
        "genesis_hash": genesis_hash,
        "no_official_mint": true,
        "mainnet_authorized": false,
        "production_keys_used": false,
        "private_keys_serialized": false,
        "candidate_config": {
            "path": candidate_path.display().to_string(),
            "sha256": candidate_sha256,
            "open_parameter_count": open_parameter_count,
            "test_values_do_not_freeze_production": true
        },
        "program": {
            "id": purpose_vault::ID.to_string(),
            "sbf_sha256": sbf_sha256,
            "sbf_bytes": sbf.len(),
            "install": "surfnet_writeProgram_local_only"
        },
        "token_program": TOKEN_PROGRAM_ID.to_string(),
        "mint": {
            "address": mint.pubkey().to_string(),
            "decimals": mint_state.decimals,
            "supply": mint_state.supply.to_string(),
            "mint_authority": Value::Null,
            "freeze_authority": Value::Null
        },
        "allocation": {
            "founder": FOUNDER.to_string(),
            "treasury": TREASURY.to_string(),
            "genesis": GENESIS.to_string(),
            "lp": LP.to_string(),
            "founder_staging_final": founder_staging.amount.to_string(),
            "treasury_staging_final": treasury_staging.amount.to_string()
        },
        "accounts": {
            "founder_staging": founder_token.pubkey().to_string(),
            "treasury_staging": treasury_token.pubkey().to_string(),
            "genesis": genesis_token.pubkey().to_string(),
            "lp": lp_token.pubkey().to_string(),
            "policy": policy.to_string(),
            "market": market.to_string(),
            "beneficiary_vault": beneficiary_vault.to_string(),
            "beneficiary_vault_token": beneficiary_vault_token.to_string(),
            "purpose_vault": purpose_vault.to_string(),
            "purpose_vault_token": purpose_vault_token.to_string(),
            "approval": approval.to_string()
        },
        "reconstruction": {
            "source": "standard getAccount RPC bytes only",
            "conserved_total": conserved.to_string(),
            "founder_vault_balance": beneficiary_vault_tokens.amount.to_string(),
            "purpose_vault_balance": purpose_vault_tokens.amount.to_string(),
            "beneficiary_released": beneficiary_destination.amount.to_string(),
            "purpose_released": contractor_destination.amount.to_string(),
            "policy_released_this_period": policy_state.released_this_period.to_string(),
            "market_eligible_volume": market_state.eligible_volume.to_string(),
            "approval_consumed": approval_state.consumed.to_string()
        },
        "expected_refusals": {
            "R3-N01": "mint after authority revocation rejected in simulation",
            "R3-N03": "beneficiary release before cliff -> CliffActive",
            "R3-N06": "joint capacity headroom plus one -> AggregateCapacityExceeded"
        },
        "transactions": signatures,
        "open_bridges": [
            "full-scale Squads member replacement and post-replacement release",
            "unrelated clean-room reproduction",
            "production parameter freeze and external review"
        ]
    });
    println!("RESULT_JSON {}", serde_json::to_string(&receipt)?);
    if let Ok(path) = env::var("K4V_R3_RECEIPT_OUT") {
        fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    }
    Ok(())
}
