// SPDX-License-Identifier: MIT OR Apache-2.0
//! Execute the B1 flow through a local Surfpool JSON-RPC endpoint.
//!
//! All signers are generated in memory and are never serialized. The program
//! is loaded with Surfpool's local-only `surfnet_writeProgram` cheatcode; mint
//! and token-account setup, deposit, and release are real signed transactions.

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use beneficiary_vault::{accounts, instruction, state::BeneficiaryVault};
use serde_json::{json, Value};
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::{config::RpcSimulateTransactionConfig, request::RpcRequest};
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
};

const DEPOSIT: u64 = 1_000_000_000;
const ANNUAL_BPS: u16 = 500;
const MONTHLY_CAP: u64 = 4_166_666;
const MIN_CLIFF_SECONDS: i64 = 730 * 24 * 60 * 60;
const POLICY_HASH: [u8; 32] = [0x42; 32];
const PAYER_LAMPORTS: u64 = 20_000_000_000;

fn program_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy/beneficiary_vault.so")
}

fn wait_for_send(stage: &str) -> Result<(), Box<dyn std::error::Error>> {
    if env::var("K4V_LOCAL_TRANSACTION_SEND_CONFIRMED").as_deref() == Ok("1") {
        println!("LOCAL_SEND_PREAUTHORIZED {stage}");
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

fn custom_rpc(
    rpc: &RpcClient,
    method: &'static str,
    params: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(rpc.send(RpcRequest::Custom { method }, params)?)
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
                    "{stage} simulation did not produce expected {needle}: err={:?}; logs={logs:?}",
                    result.err
                )
                .into());
            }
            println!(
                "SIMULATION {stage} EXPECTED_REJECTION log={needle} units_consumed={}",
                result.units_consumed.unwrap_or_default()
            );
        }
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

fn token_amount(rpc: &RpcClient, address: &Pubkey) -> Result<u64, Box<dyn std::error::Error>> {
    let account = rpc.get_account(address)?;
    Ok(SplAccount::unpack(&account.data)?.amount)
}

fn read_state(
    rpc: &RpcClient,
    address: &Pubkey,
) -> Result<BeneficiaryVault, Box<dyn std::error::Error>> {
    let account = rpc.get_account(address)?;
    Ok(BeneficiaryVault::try_deserialize(
        &mut account.data.as_slice(),
    )?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rpc_url =
        env::var("K4V_SURFPOOL_RPC").unwrap_or_else(|_| "http://127.0.0.1:18999".to_string());
    let rpc = RpcClient::new(rpc_url.clone());
    let version = rpc.get_version()?;
    if !rpc_url.starts_with("http://127.0.0.1:") && !rpc_url.starts_with("http://localhost:") {
        return Err("transaction probe is restricted to a loopback RPC URL".into());
    }
    println!("CLUSTER local-surfpool rpc={rpc_url} version={version:?}");

    let program_bytes = fs::read(program_path())?;
    let _: Value = custom_rpc(
        &rpc,
        "surfnet_writeProgram",
        json!([
            beneficiary_vault::ID.to_string(),
            hex::encode(&program_bytes),
            0
        ]),
    )?;
    let program_account = rpc.get_account(&beneficiary_vault::ID)?;
    if !program_account.executable {
        return Err("Surfpool did not expose the loaded B1 program as executable".into());
    }
    println!(
        "PROGRAM_LOADED id={} bytes={} executable=true",
        beneficiary_vault::ID,
        program_path().metadata()?.len()
    );

    let payer = Keypair::new();
    let beneficiary = Keypair::new();
    let mint = Keypair::new();
    let depositor_token = Keypair::new();
    let beneficiary_token = Keypair::new();
    let _: Value = custom_rpc(
        &rpc,
        "surfnet_setAccount",
        json!([
            payer.pubkey().to_string(),
            {
                "lamports": PAYER_LAMPORTS,
                "owner": system_program::ID.to_string(),
                "executable": false
            }
        ]),
    )?;
    if rpc.get_balance(&payer.pubkey())? != PAYER_LAMPORTS {
        return Err("ephemeral payer funding did not reconcile".into());
    }

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
    simulate(&rpc, &setup_tx, "SETUP", None)?;
    wait_for_send("SETUP")?;
    let setup_signature = rpc.send_and_confirm_transaction(&setup_tx)?;
    if token_amount(&rpc, &depositor_token.pubkey())? != DEPOSIT {
        return Err("setup token amount did not reconcile".into());
    }
    println!("TRANSACTION SETUP CONFIRMED signature={setup_signature}");

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
    simulate(&rpc, &deposit_tx, "DEPOSIT", None)?;
    wait_for_send("DEPOSIT")?;
    let deposit_signature = rpc.send_and_confirm_transaction(&deposit_tx)?;
    let deposited_state = read_state(&rpc, &vault_state)?;
    if deposited_state.deposited_amount != DEPOSIT
        || deposited_state.monthly_cap != MONTHLY_CAP
        || token_amount(&rpc, &vault_token)? != DEPOSIT
        || token_amount(&rpc, &depositor_token.pubkey())? != 0
    {
        return Err("deposit state did not reconcile".into());
    }
    println!("TRANSACTION DEPOSIT CONFIRMED signature={deposit_signature}");

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
    simulate(
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
    let release_tx = signed_transaction(&rpc, &[release_ix], &payer, &[&beneficiary])?;
    simulate(&rpc, &release_tx, "RELEASE", None)?;
    wait_for_send("RELEASE")?;
    let release_signature = rpc.send_and_confirm_transaction(&release_tx)?;

    let released_state = read_state(&rpc, &vault_state)?;
    let vault_balance = token_amount(&rpc, &vault_token)?;
    let beneficiary_balance = token_amount(&rpc, &beneficiary_token.pubkey())?;
    if released_state.released_total != MONTHLY_CAP
        || released_state.released_this_period != MONTHLY_CAP
        || vault_balance != DEPOSIT - MONTHLY_CAP
        || beneficiary_balance != MONTHLY_CAP
    {
        return Err("release state did not reconcile".into());
    }
    println!("TRANSACTION RELEASE CONFIRMED signature={release_signature}");
    println!(
        "RESULT_JSON {}",
        serde_json::to_string(&json!({
            "schema": "k4v-b1-rpc-transaction-probe/v0.1",
            "cluster": "local-surfpool",
            "rpc_url": rpc_url,
            "program_id": beneficiary_vault::ID.to_string(),
            "program_install": "surfnet_writeProgram",
            "classic_spl_token": true,
            "ephemeral_signers_only": true,
            "private_keys_serialized": false,
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
            "signatures": {
                "setup": setup_signature.to_string(),
                "deposit": deposit_signature.to_string(),
                "release": release_signature.to_string()
            }
        }))?
    );
    Ok(())
}
