//! B2 integration tests against the loaded SBF artifact.
//!
//! Every test here maps to a rejection vector in
//! `spec/PURPOSE_BOUND_VAULT_COVENANT.md` or to a property B2 adds over B1:
//! the shared capacity window, the approved need, the notice period, the
//! recusal rule, and a market input that fails closed.

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use litesvm::LiteSVM;
use purpose_vault::{
    accounts,
    constants::{
        APPROVAL_SEED, MARKET_SEED, MIN_CLIFF_SECONDS, MIN_SILENCE_GRACE_SECONDS,
        ORACLE_ROTATION_NOTICE_SECONDS, PERIOD_SECONDS, POLICY_SEED, TOKEN_VAULT_SEED, VAULT_SEED,
    },
    instruction,
    state::{Approval, CovenantVault, MarketInput, PolicyWindow, VaultKind},
};
use solana_account::Account;
use solana_clock::Clock;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_program_option::COption;
use solana_program_pack::Pack;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;
use solana_transaction::Transaction;
use spl_token_interface::{
    instruction::{self as token_instruction, AuthorityType},
    state::{Account as SplAccount, AccountState, Mint},
    ID as TOKEN_PROGRAM_ID,
};
use std::path::PathBuf;

const BENEFICIARY_DEPOSIT: u64 = 300_000_000;
const PURPOSE_DEPOSIT: u64 = 500_000_000;
const GENESIS_ALLOCATION: u64 = 0;
const LP_ALLOCATION: u64 = 0;
const ANNUAL_BPS: u16 = 500;
const BENEFICIARY_CAP: u64 = 1_250_000;
const PURPOSE_CAP: u64 = 2_083_333;
const MARKET_BPS: u16 = 250;
const ELIGIBLE_VOLUME: u64 = 120_000_000;
const MARKET_CAPACITY: u64 = 3_000_000;
const FULL_BENEFICIARY_DEPOSIT: u64 = 300_000_000_000_000_000;
const FULL_PURPOSE_DEPOSIT: u64 = 500_000_000_000_000_000;
const FULL_GENESIS_ALLOCATION: u64 = 120_000_000_000_000_000;
const FULL_LP_ALLOCATION: u64 = 80_000_000_000_000_000;
const FULL_TOTAL_SUPPLY: u64 = 1_000_000_000_000_000_000;
const FULL_BENEFICIARY_CAP: u64 = 1_250_000_000_000_000;
const FULL_PURPOSE_CAP: u64 = 2_083_333_333_333_333;
const FULL_ELIGIBLE_VOLUME: u64 = 120_000_000_000_000_000;
const FULL_MARKET_CAPACITY: u64 = 3_000_000_000_000_000;
const MAX_AGE_SECONDS: i64 = 3 * 24 * 60 * 60;
const POLICY_HASH: [u8; 32] = [0x42; 32];
/// What the two frozen vault schedules permit in one period between them. No
/// oracle report can raise it, because no oracle touches a vault's cap.
const SUM_OF_MONTHLY_CAPS: u64 = BENEFICIARY_CAP + PURPOSE_CAP;
const NO_CEILING: u64 = u64::MAX;
const SILENCE_FLOOR: u64 = 30_000;
const SILENCE_GRACE: i64 = 180 * 24 * 60 * 60;
const ROTATION_NOTICE: i64 = 90 * 24 * 60 * 60;
const LOW_CEILING: u64 = 1_500_000;
/// A report inflated by six orders of magnitude, as a captured oracle would.
const INFLATED_VOLUME: u64 = ELIGIBLE_VOLUME * 1_000_000;

/// The premise of the headline test, checked at compile time: each vault fits
/// under the market ceiling on its own, and the two together do not.
const _: () = assert!(BENEFICIARY_CAP <= MARKET_CAPACITY);
const _: () = assert!(PURPOSE_CAP <= MARKET_CAPACITY);
const _: () = assert!(BENEFICIARY_CAP + PURPOSE_CAP > MARKET_CAPACITY);
/// The premises of the two ceiling tests: a low ceiling must bind before the
/// market term does, and an inflated report must widen the window past it.
const _: () = assert!(LOW_CEILING < MARKET_CAPACITY);
const _: () = assert!(BENEFICIARY_CAP < LOW_CEILING);
const _: () = assert!(MARKET_CAPACITY < SUM_OF_MONTHLY_CAPS);
/// The two frozen delays this file pins against the program's own constants.
const _: () = assert!(SILENCE_GRACE == MIN_SILENCE_GRACE_SECONDS);
const _: () = assert!(ROTATION_NOTICE == ORACLE_ROTATION_NOTICE_SECONDS);
/// A trickle, not an income: the floor must be far under a vault's own cap, or
/// silence would become a way to release at the ordinary rate.
const _: () = assert!(SILENCE_FLOOR * 40 < BENEFICIARY_CAP);
/// Replacing a lost oracle has to be the faster path, or nobody would use it.
const _: () = assert!(ROTATION_NOTICE < SILENCE_GRACE);
const _: () = assert!(
    FULL_BENEFICIARY_DEPOSIT + FULL_PURPOSE_DEPOSIT + FULL_GENESIS_ALLOCATION + FULL_LP_ALLOCATION
        == FULL_TOTAL_SUPPLY
);
const _: () = assert!(FULL_BENEFICIARY_CAP <= FULL_MARKET_CAPACITY);
const _: () = assert!(FULL_PURPOSE_CAP <= FULL_MARKET_CAPACITY);
const _: () = assert!(FULL_BENEFICIARY_CAP + FULL_PURPOSE_CAP > FULL_MARKET_CAPACITY);
/// Probe-C pins: six purpose-first periods must remain inside the deposited
/// principal, and the published devnet ceiling must squeeze the beneficiary
/// without zeroing it.
const DEVNET_HARD_CEILING: u64 = 2_500_000;
const DEVNET_SQUEEZE_HEADROOM: u64 = DEVNET_HARD_CEILING - PURPOSE_CAP;
const _: () = assert!(6 * PURPOSE_CAP < PURPOSE_DEPOSIT);
const _: () = assert!(DEVNET_HARD_CEILING > PURPOSE_CAP);
const _: () = assert!(DEVNET_HARD_CEILING < BENEFICIARY_CAP + PURPOSE_CAP);
const _: () = assert!(DEVNET_SQUEEZE_HEADROOM == 416_667);
const _: () = assert!(DEVNET_SQUEEZE_HEADROOM < BENEFICIARY_CAP);

#[derive(Clone, Copy)]
struct FixtureAmounts {
    decimals: u8,
    beneficiary_deposit: u64,
    purpose_deposit: u64,
    genesis_allocation: u64,
    lp_allocation: u64,
    beneficiary_cap: u64,
    purpose_cap: u64,
    eligible_volume: u64,
    market_capacity: u64,
}

const SCALED_AMOUNTS: FixtureAmounts = FixtureAmounts {
    decimals: 9,
    beneficiary_deposit: BENEFICIARY_DEPOSIT,
    purpose_deposit: PURPOSE_DEPOSIT,
    genesis_allocation: GENESIS_ALLOCATION,
    lp_allocation: LP_ALLOCATION,
    beneficiary_cap: BENEFICIARY_CAP,
    purpose_cap: PURPOSE_CAP,
    eligible_volume: ELIGIBLE_VOLUME,
    market_capacity: MARKET_CAPACITY,
};

const FULL_SCALE_AMOUNTS: FixtureAmounts = FixtureAmounts {
    decimals: 9,
    beneficiary_deposit: FULL_BENEFICIARY_DEPOSIT,
    purpose_deposit: FULL_PURPOSE_DEPOSIT,
    genesis_allocation: FULL_GENESIS_ALLOCATION,
    lp_allocation: FULL_LP_ALLOCATION,
    beneficiary_cap: FULL_BENEFICIARY_CAP,
    purpose_cap: FULL_PURPOSE_CAP,
    eligible_volume: FULL_ELIGIBLE_VOLUME,
    market_capacity: FULL_MARKET_CAPACITY,
};

struct Fixture {
    svm: LiteSVM,
    depositor: Keypair,
    policy_authority: Keypair,
    oracle: Keypair,
    beneficiary: Keypair,
    approver: Keypair,
    mint: Pubkey,
    depositor_token: Pubkey,
    beneficiary_token: Pubkey,
    contractor_token: Pubkey,
    approver_token: Pubkey,
    genesis_token: Pubkey,
    lp_token: Pubkey,
    policy: Pubkey,
    market: Pubkey,
    beneficiary_vault: Pubkey,
    beneficiary_vault_token: Pubkey,
    purpose_vault: Pubkey,
    purpose_vault_token: Pubkey,
    genesis_ts: i64,
    amounts: FixtureAmounts,
}

fn program_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy/purpose_vault.so")
}

fn token_account(mint: Pubkey, owner: Pubkey, amount: u64) -> Account {
    let value = SplAccount {
        mint,
        owner,
        amount,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    let mut data = vec![0; SplAccount::LEN];
    SplAccount::pack(value, &mut data).unwrap();
    Account {
        lamports: 10_000_000,
        data,
        owner: TOKEN_PROGRAM_ID,
        executable: false,
        rent_epoch: 0,
    }
}

/// The instruction and the signers come first so that every read-only borrow of
/// the fixture is evaluated before the mutable borrow of the SVM.
fn send(
    instruction: Instruction,
    signers: &[&Keypair],
    svm: &mut LiteSVM,
) -> Result<litesvm::types::TransactionMetadata, Box<litesvm::types::FailedTransactionMetadata>> {
    send_instructions(&[instruction], signers, svm)
}

fn send_instructions(
    instructions: &[Instruction],
    signers: &[&Keypair],
    svm: &mut LiteSVM,
) -> Result<litesvm::types::TransactionMetadata, Box<litesvm::types::FailedTransactionMetadata>> {
    svm.expire_blockhash();
    let payer = signers[0];
    svm.send_transaction(Transaction::new_signed_with_payer(
        instructions,
        Some(&payer.pubkey()),
        signers,
        svm.latest_blockhash(),
    ))
    .map_err(Box::new)
}

fn assert_failed_with(
    outcome: Result<
        litesvm::types::TransactionMetadata,
        Box<litesvm::types::FailedTransactionMetadata>,
    >,
    needle: &str,
) {
    let failure = outcome.expect_err("expected this transaction to be rejected");
    assert!(
        failure.meta.logs.iter().any(|line| line.contains(needle)),
        "expected {needle} in logs, got:\n{}",
        failure.meta.logs.join("\n")
    );
}

fn policy_pda() -> Pubkey {
    Pubkey::find_program_address(&[POLICY_SEED, POLICY_HASH.as_ref()], &purpose_vault::ID).0
}

fn market_pda() -> Pubkey {
    Pubkey::find_program_address(&[MARKET_SEED, POLICY_HASH.as_ref()], &purpose_vault::ID).0
}

fn vault_pda(kind: VaultKind, authority: Pubkey, mint: Pubkey) -> (Pubkey, Pubkey) {
    let vault = Pubkey::find_program_address(
        &[
            VAULT_SEED,
            POLICY_HASH.as_ref(),
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

/// How a policy is opened. Everything but `report_volume` is frozen at
/// creation and has no instruction that can change it afterwards.
#[derive(Clone, Copy)]
struct PolicyConfig {
    report_volume: bool,
    hard_ceiling: u64,
    silence_floor: u64,
    silence_grace_seconds: i64,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            report_volume: true,
            hard_ceiling: NO_CEILING,
            silence_floor: 0,
            silence_grace_seconds: 0,
        }
    }
}

fn open_policy_instruction(
    authority: Pubkey,
    oracle: Pubkey,
    mint: Pubkey,
    market_capacity_bps: u16,
    max_age_seconds: i64,
    config: PolicyConfig,
) -> Instruction {
    Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::OpenPolicy {
            authority,
            oracle,
            mint,
            policy: policy_pda(),
            market: market_pda(),
            system_program: solana_system_interface::program::ID,
        }
        .to_account_metas(None),
        data: instruction::OpenPolicy {
            policy_hash: POLICY_HASH,
            market_capacity_bps,
            max_age_seconds,
            hard_ceiling: config.hard_ceiling,
            silence_floor: config.silence_floor,
            silence_grace_seconds: config.silence_grace_seconds,
        }
        .data(),
    }
}

fn propose_oracle_instruction(policy_authority: Pubkey, new_oracle: Pubkey) -> Instruction {
    Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ProposeOracle {
            policy_authority,
            policy: policy_pda(),
            market: market_pda(),
        }
        .to_account_metas(None),
        data: instruction::ProposeOracle { new_oracle }.data(),
    }
}

fn execute_rotation_instruction() -> Instruction {
    Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ExecuteOracleRotation {
            market: market_pda(),
        }
        .to_account_metas(None),
        data: instruction::ExecuteOracleRotation {}.data(),
    }
}

fn report_instruction(oracle: Pubkey, eligible_volume: u64) -> Instruction {
    Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ReportVolume {
            oracle,
            market: market_pda(),
        }
        .to_account_metas(None),
        data: instruction::ReportVolume { eligible_volume }.data(),
    }
}

struct DepositArgs {
    kind: VaultKind,
    amount: u64,
    annual_release_bps: u16,
    cliff_seconds: i64,
}

fn deposit_instruction(
    depositor: Pubkey,
    policy_authority: Pubkey,
    authority: Pubkey,
    mint: Pubkey,
    depositor_token: Pubkey,
    args: DepositArgs,
) -> (Instruction, Pubkey, Pubkey) {
    let (vault, vault_token) = vault_pda(args.kind, authority, mint);
    (
        Instruction {
            program_id: purpose_vault::ID,
            accounts: accounts::Deposit {
                depositor,
                policy_authority,
                authority,
                mint,
                depositor_token,
                policy: policy_pda(),
                vault,
                vault_token,
                token_program: TOKEN_PROGRAM_ID,
                system_program: solana_system_interface::program::ID,
            }
            .to_account_metas(None),
            data: instruction::Deposit {
                kind: args.kind,
                amount: args.amount,
                annual_release_bps: args.annual_release_bps,
                cliff_seconds: args.cliff_seconds,
            }
            .data(),
        },
        vault,
        vault_token,
    )
}

fn approve_instruction(
    approver: Pubkey,
    mint: Pubkey,
    vault: Pubkey,
    destination: Pubkey,
    period_index: u64,
    approved_need: u64,
) -> Instruction {
    Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::Approve {
            approver,
            mint,
            vault,
            policy: policy_pda(),
            destination,
            approval: approval_pda(vault, period_index),
            system_program: solana_system_interface::program::ID,
        }
        .to_account_metas(None),
        data: instruction::Approve {
            period_index,
            approved_need,
        }
        .data(),
    }
}

fn release_purpose_instruction(
    fixture: &Fixture,
    destination: Pubkey,
    period_index: u64,
    amount: u64,
) -> Instruction {
    Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ReleasePurpose {
            approver: fixture.approver.pubkey(),
            mint: fixture.mint,
            vault: fixture.purpose_vault,
            policy: fixture.policy,
            market: fixture.market,
            vault_token: fixture.purpose_vault_token,
            destination,
            approval: approval_pda(fixture.purpose_vault, period_index),
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::ReleasePurpose { amount }.data(),
    }
}

fn release_beneficiary_instruction(fixture: &Fixture, amount: u64) -> Instruction {
    Instruction {
        program_id: purpose_vault::ID,
        accounts: accounts::ReleaseBeneficiary {
            beneficiary: fixture.beneficiary.pubkey(),
            mint: fixture.mint,
            vault: fixture.beneficiary_vault,
            policy: fixture.policy,
            market: fixture.market,
            vault_token: fixture.beneficiary_vault_token,
            destination: fixture.beneficiary_token,
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::ReleaseBeneficiary { amount }.data(),
    }
}

fn read_policy(fixture: &Fixture) -> PolicyWindow {
    let account = fixture.svm.get_account(&fixture.policy).unwrap();
    PolicyWindow::try_deserialize(&mut account.data.as_slice()).unwrap()
}

fn read_market(fixture: &Fixture) -> MarketInput {
    let account = fixture.svm.get_account(&fixture.market).unwrap();
    MarketInput::try_deserialize(&mut account.data.as_slice()).unwrap()
}

fn read_vault(fixture: &Fixture, vault: Pubkey) -> CovenantVault {
    let account = fixture.svm.get_account(&vault).unwrap();
    CovenantVault::try_deserialize(&mut account.data.as_slice()).unwrap()
}

fn read_approval(fixture: &Fixture, vault: Pubkey, period_index: u64) -> Approval {
    let account = fixture
        .svm
        .get_account(&approval_pda(vault, period_index))
        .unwrap();
    Approval::try_deserialize(&mut account.data.as_slice()).unwrap()
}

fn token_balance(svm: &LiteSVM, address: Pubkey) -> u64 {
    let account = svm.get_account(&address).unwrap();
    SplAccount::unpack(&account.data).unwrap().amount
}

fn set_time(unix_timestamp: i64, fixture: &mut Fixture) {
    let mut clock = fixture.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = unix_timestamp;
    fixture.svm.set_sysvar(&clock);
}

fn period_start(fixture: &Fixture, period_index: u64) -> i64 {
    fixture.genesis_ts + PERIOD_SECONDS * period_index as i64
}

/// The first period that is both after the beneficiary cliff and far enough
/// past an approval written at the cliff to clear the 30-day notice.
fn period_at(fixture: &Fixture, unix_timestamp: i64) -> u64 {
    ((unix_timestamp - fixture.genesis_ts) / PERIOD_SECONDS) as u64
}

fn first_joint_period() -> u64 {
    let cliff_period = (MIN_CLIFF_SECONDS / PERIOD_SECONDS) as u64;
    cliff_period + 2
}

fn setup_amounts_with(config: PolicyConfig, amounts: FixtureAmounts) -> Fixture {
    setup_amounts_with_freeze(config, amounts, COption::None)
}

fn setup_amounts_with_freeze(
    config: PolicyConfig,
    amounts: FixtureAmounts,
    freeze_authority: COption<Pubkey>,
) -> Fixture {
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(purpose_vault::ID, program_path())
        .unwrap();

    let depositor = Keypair::new();
    let policy_authority = Keypair::new();
    let oracle = Keypair::new();
    let beneficiary = Keypair::new();
    let approver = Keypair::new();
    let contractor = Keypair::new();
    for key in [
        &depositor,
        &policy_authority,
        &oracle,
        &beneficiary,
        &approver,
        &contractor,
    ] {
        svm.airdrop(&key.pubkey(), 10_000_000_000).unwrap();
    }

    let mint = Pubkey::new_unique();
    let mint_value = Mint {
        mint_authority: COption::None,
        supply: amounts.beneficiary_deposit
            + amounts.purpose_deposit
            + amounts.genesis_allocation
            + amounts.lp_allocation,
        decimals: amounts.decimals,
        is_initialized: true,
        freeze_authority,
    };
    let mut mint_data = vec![0; Mint::LEN];
    Mint::pack(mint_value, &mut mint_data).unwrap();
    svm.set_account(
        mint,
        Account {
            lamports: 10_000_000,
            data: mint_data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let depositor_token = Pubkey::new_unique();
    let beneficiary_token = Pubkey::new_unique();
    let contractor_token = Pubkey::new_unique();
    let approver_token = Pubkey::new_unique();
    let genesis_token = Pubkey::new_unique();
    let lp_token = Pubkey::new_unique();
    svm.set_account(
        depositor_token,
        token_account(
            mint,
            depositor.pubkey(),
            amounts.beneficiary_deposit + amounts.purpose_deposit,
        ),
    )
    .unwrap();
    svm.set_account(
        beneficiary_token,
        token_account(mint, beneficiary.pubkey(), 0),
    )
    .unwrap();
    svm.set_account(
        contractor_token,
        token_account(mint, contractor.pubkey(), 0),
    )
    .unwrap();
    svm.set_account(approver_token, token_account(mint, approver.pubkey(), 0))
        .unwrap();
    svm.set_account(
        genesis_token,
        token_account(mint, Pubkey::new_unique(), amounts.genesis_allocation),
    )
    .unwrap();
    svm.set_account(
        lp_token,
        token_account(mint, Pubkey::new_unique(), amounts.lp_allocation),
    )
    .unwrap();

    send(
        open_policy_instruction(
            policy_authority.pubkey(),
            oracle.pubkey(),
            mint,
            MARKET_BPS,
            MAX_AGE_SECONDS,
            config,
        ),
        &[&policy_authority],
        &mut svm,
    )
    .unwrap();

    let (beneficiary_deposit, beneficiary_vault, beneficiary_vault_token) = deposit_instruction(
        depositor.pubkey(),
        policy_authority.pubkey(),
        beneficiary.pubkey(),
        mint,
        depositor_token,
        DepositArgs {
            kind: VaultKind::Beneficiary,
            amount: amounts.beneficiary_deposit,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
        },
    );
    send(
        beneficiary_deposit,
        &[&depositor, &policy_authority],
        &mut svm,
    )
    .unwrap();

    let (purpose_deposit, purpose_vault, purpose_vault_token) = deposit_instruction(
        depositor.pubkey(),
        policy_authority.pubkey(),
        approver.pubkey(),
        mint,
        depositor_token,
        DepositArgs {
            kind: VaultKind::Purpose,
            amount: amounts.purpose_deposit,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: 0,
        },
    );
    send(purpose_deposit, &[&depositor, &policy_authority], &mut svm).unwrap();

    if config.report_volume {
        send(
            report_instruction(oracle.pubkey(), amounts.eligible_volume),
            &[&oracle],
            &mut svm,
        )
        .unwrap();
    }

    let policy = policy_pda();
    let genesis_ts = {
        let account = svm.get_account(&policy).unwrap();
        PolicyWindow::try_deserialize(&mut account.data.as_slice())
            .unwrap()
            .genesis_ts
    };

    Fixture {
        svm,
        depositor,
        policy_authority,
        oracle,
        beneficiary,
        approver,
        mint,
        depositor_token,
        beneficiary_token,
        contractor_token,
        approver_token,
        genesis_token,
        lp_token,
        policy,
        market: market_pda(),
        beneficiary_vault,
        beneficiary_vault_token,
        purpose_vault,
        purpose_vault_token,
        genesis_ts,
        amounts,
    }
}

fn setup_with(config: PolicyConfig) -> Fixture {
    setup_amounts_with(config, SCALED_AMOUNTS)
}

fn setup() -> Fixture {
    setup_with(PolicyConfig::default())
}

fn create_transaction_token_account(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint: Pubkey,
    owner: Pubkey,
) -> Pubkey {
    let account = Keypair::new();
    send_instructions(
        &[
            system_instruction::create_account(
                &payer.pubkey(),
                &account.pubkey(),
                10_000_000,
                SplAccount::LEN as u64,
                &TOKEN_PROGRAM_ID,
            ),
            token_instruction::initialize_account3(
                &TOKEN_PROGRAM_ID,
                &account.pubkey(),
                &mint,
                &owner,
            )
            .unwrap(),
        ],
        &[payer, &account],
        svm,
    )
    .unwrap();
    account.pubkey()
}

/// R3-B construction: unlike `setup_amounts_with`, every mint and token account
/// in this fixture is created by signed System/SPL transactions. The four
/// allocations are minted separately, both mint authorities are then revoked,
/// and the exact Founder/Treasury accounts continue into B2 without replacing
/// the mint or injecting account bytes.
fn setup_transaction_created_full_scale() -> (Fixture, Pubkey) {
    let amounts = FULL_SCALE_AMOUNTS;
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(purpose_vault::ID, program_path())
        .unwrap();

    let depositor = Keypair::new();
    let policy_authority = Keypair::new();
    let oracle = Keypair::new();
    let beneficiary = Keypair::new();
    let approver = Keypair::new();
    let contractor = Keypair::new();
    for key in [
        &depositor,
        &policy_authority,
        &oracle,
        &beneficiary,
        &approver,
        &contractor,
    ] {
        svm.airdrop(&key.pubkey(), 10_000_000_000).unwrap();
    }

    let mint_keypair = Keypair::new();
    let mint = mint_keypair.pubkey();
    send_instructions(
        &[
            system_instruction::create_account(
                &depositor.pubkey(),
                &mint,
                10_000_000,
                Mint::LEN as u64,
                &TOKEN_PROGRAM_ID,
            ),
            token_instruction::initialize_mint2(
                &TOKEN_PROGRAM_ID,
                &mint,
                &depositor.pubkey(),
                Some(&depositor.pubkey()),
                amounts.decimals,
            )
            .unwrap(),
        ],
        &[&depositor, &mint_keypair],
        &mut svm,
    )
    .unwrap();

    let founder_token =
        create_transaction_token_account(&mut svm, &depositor, mint, depositor.pubkey());
    let treasury_token =
        create_transaction_token_account(&mut svm, &depositor, mint, depositor.pubkey());
    let genesis_token =
        create_transaction_token_account(&mut svm, &depositor, mint, Pubkey::new_unique());
    let lp_token =
        create_transaction_token_account(&mut svm, &depositor, mint, Pubkey::new_unique());
    let beneficiary_token =
        create_transaction_token_account(&mut svm, &depositor, mint, beneficiary.pubkey());
    let contractor_token =
        create_transaction_token_account(&mut svm, &depositor, mint, contractor.pubkey());
    let approver_token =
        create_transaction_token_account(&mut svm, &depositor, mint, approver.pubkey());

    let allocation_instructions = [
        (founder_token, amounts.beneficiary_deposit),
        (treasury_token, amounts.purpose_deposit),
        (genesis_token, amounts.genesis_allocation),
        (lp_token, amounts.lp_allocation),
    ]
    .into_iter()
    .map(|(account, amount)| {
        token_instruction::mint_to(
            &TOKEN_PROGRAM_ID,
            &mint,
            &account,
            &depositor.pubkey(),
            &[],
            amount,
        )
        .unwrap()
    })
    .collect::<Vec<_>>();
    send_instructions(&allocation_instructions, &[&depositor], &mut svm).unwrap();

    let created_mint = Mint::unpack(&svm.get_account(&mint).unwrap().data).unwrap();
    assert_eq!(created_mint.supply, FULL_TOTAL_SUPPLY);
    assert_eq!(
        created_mint.mint_authority,
        COption::Some(depositor.pubkey())
    );
    assert_eq!(
        created_mint.freeze_authority,
        COption::Some(depositor.pubkey())
    );
    assert_eq!(
        token_balance(&svm, founder_token),
        amounts.beneficiary_deposit
    );
    assert_eq!(token_balance(&svm, treasury_token), amounts.purpose_deposit);
    assert_eq!(
        token_balance(&svm, genesis_token),
        amounts.genesis_allocation
    );
    assert_eq!(token_balance(&svm, lp_token), amounts.lp_allocation);

    send_instructions(
        &[
            token_instruction::set_authority(
                &TOKEN_PROGRAM_ID,
                &mint,
                None,
                AuthorityType::MintTokens,
                &depositor.pubkey(),
                &[],
            )
            .unwrap(),
            token_instruction::set_authority(
                &TOKEN_PROGRAM_ID,
                &mint,
                None,
                AuthorityType::FreezeAccount,
                &depositor.pubkey(),
                &[],
            )
            .unwrap(),
        ],
        &[&depositor],
        &mut svm,
    )
    .unwrap();

    let revoked_mint = Mint::unpack(&svm.get_account(&mint).unwrap().data).unwrap();
    assert_eq!(revoked_mint.supply, FULL_TOTAL_SUPPLY);
    assert_eq!(revoked_mint.mint_authority, COption::None);
    assert_eq!(revoked_mint.freeze_authority, COption::None);

    // R3-N01: the former authority cannot mint one more base unit after
    // revocation, and a rejected transaction must not change supply.
    let mint_one_more = token_instruction::mint_to(
        &TOKEN_PROGRAM_ID,
        &mint,
        &beneficiary_token,
        &depositor.pubkey(),
        &[],
        1,
    )
    .unwrap();
    assert!(send(mint_one_more, &[&depositor], &mut svm).is_err());
    assert_eq!(
        Mint::unpack(&svm.get_account(&mint).unwrap().data)
            .unwrap()
            .supply,
        FULL_TOTAL_SUPPLY
    );

    send(
        open_policy_instruction(
            policy_authority.pubkey(),
            oracle.pubkey(),
            mint,
            MARKET_BPS,
            MAX_AGE_SECONDS,
            PolicyConfig::default(),
        ),
        &[&policy_authority],
        &mut svm,
    )
    .unwrap();

    let (beneficiary_deposit, beneficiary_vault, beneficiary_vault_token) = deposit_instruction(
        depositor.pubkey(),
        policy_authority.pubkey(),
        beneficiary.pubkey(),
        mint,
        founder_token,
        DepositArgs {
            kind: VaultKind::Beneficiary,
            amount: amounts.beneficiary_deposit,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
        },
    );
    send(
        beneficiary_deposit,
        &[&depositor, &policy_authority],
        &mut svm,
    )
    .unwrap();

    let (purpose_deposit, purpose_vault, purpose_vault_token) = deposit_instruction(
        depositor.pubkey(),
        policy_authority.pubkey(),
        approver.pubkey(),
        mint,
        treasury_token,
        DepositArgs {
            kind: VaultKind::Purpose,
            amount: amounts.purpose_deposit,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: 0,
        },
    );
    send(purpose_deposit, &[&depositor, &policy_authority], &mut svm).unwrap();
    send(
        report_instruction(oracle.pubkey(), amounts.eligible_volume),
        &[&oracle],
        &mut svm,
    )
    .unwrap();

    let policy = policy_pda();
    let genesis_ts = {
        let account = svm.get_account(&policy).unwrap();
        PolicyWindow::try_deserialize(&mut account.data.as_slice())
            .unwrap()
            .genesis_ts
    };

    (
        Fixture {
            svm,
            depositor,
            policy_authority,
            oracle,
            beneficiary,
            approver,
            mint,
            depositor_token: founder_token,
            beneficiary_token,
            contractor_token,
            approver_token,
            genesis_token,
            lp_token,
            policy,
            market: market_pda(),
            beneficiary_vault,
            beneficiary_vault_token,
            purpose_vault,
            purpose_vault_token,
            genesis_ts,
            amounts,
        },
        treasury_token,
    )
}

/// Approve at `approve_at`, then move to the start of `period_index`.
/// The oracle must speak again after any jump longer than the tolerance. That
/// is what the tolerance is for, so a test that moves a whole period forward
/// has to refresh rather than assume the old number still counts.
fn refresh_market(fixture: &mut Fixture) {
    report_volume(fixture.amounts.eligible_volume, fixture);
}

fn report_volume(eligible_volume: u64, fixture: &mut Fixture) {
    let oracle = fixture.oracle.insecure_clone();
    send(
        report_instruction(oracle.pubkey(), eligible_volume),
        &[&oracle],
        &mut fixture.svm,
    )
    .unwrap();
}

fn approve_and_advance(
    approve_at: i64,
    period_index: u64,
    approved_need: u64,
    destination: Pubkey,
    fixture: &mut Fixture,
) {
    set_time(approve_at, fixture);
    let approver = fixture.approver.insecure_clone();
    send(
        approve_instruction(
            approver.pubkey(),
            fixture.mint,
            fixture.purpose_vault,
            destination,
            period_index,
            approved_need,
        ),
        &[&approver],
        &mut fixture.svm,
    )
    .unwrap();
    let target = period_start(fixture, period_index);
    set_time(target, fixture);
}

#[test]
fn deposit_freezes_two_kinds_against_one_shared_window() {
    let fixture = setup();

    let policy = read_policy(&fixture);
    assert_eq!(policy.authority, fixture.policy_authority.pubkey());
    assert_eq!(policy.mint, fixture.mint);
    assert_eq!(policy.policy_hash, POLICY_HASH);
    assert_eq!(policy.vault_count, 2);
    assert_eq!(policy.released_this_period, 0);
    assert_eq!(policy.current_period_index, 0);
    assert_eq!(policy.hard_ceiling, NO_CEILING);

    let market = read_market(&fixture);
    assert_eq!(market.oracle, fixture.oracle.pubkey());
    assert_eq!(market.market_capacity_bps, MARKET_BPS);
    assert_eq!(market.max_age_seconds, MAX_AGE_SECONDS);
    assert_eq!(market.eligible_volume, ELIGIBLE_VOLUME);
    assert_eq!(market.report_count, 1);

    let beneficiary = read_vault(&fixture, fixture.beneficiary_vault);
    assert_eq!(beneficiary.kind, VaultKind::Beneficiary);
    assert_eq!(beneficiary.authority, fixture.beneficiary.pubkey());
    assert_eq!(beneficiary.deposited_amount, BENEFICIARY_DEPOSIT);
    assert_eq!(beneficiary.monthly_cap, BENEFICIARY_CAP);
    assert_eq!(
        beneficiary.cliff_end_ts - beneficiary.genesis_ts,
        MIN_CLIFF_SECONDS
    );

    let purpose = read_vault(&fixture, fixture.purpose_vault);
    assert_eq!(purpose.kind, VaultKind::Purpose);
    assert_eq!(purpose.authority, fixture.approver.pubkey());
    assert_eq!(purpose.deposited_amount, PURPOSE_DEPOSIT);
    assert_eq!(purpose.monthly_cap, PURPOSE_CAP);
    assert_eq!(purpose.cliff_end_ts, purpose.genesis_ts);

    assert_eq!(token_balance(&fixture.svm, fixture.depositor_token), 0);
    assert_eq!(
        token_balance(&fixture.svm, fixture.beneficiary_vault_token),
        BENEFICIARY_DEPOSIT
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.purpose_vault_token),
        PURPOSE_DEPOSIT
    );
}

#[test]
fn full_scale_nine_decimal_b2_graph_reconciles_and_enforces_shared_capacity() {
    // R3-A: exercise the compiled B2 SBF at the actual 1B-whole-token scale on
    // one classic-SPL mint. The mint and token accounts are injected fixtures,
    // so this closes the B2 amount/accounting slice, not the transaction-level
    // mint/allocation/authority-revocation slice of the full R3 specification.
    let mut fixture = setup_amounts_with(PolicyConfig::default(), FULL_SCALE_AMOUNTS);
    let amounts = fixture.amounts;

    let mint_account = fixture.svm.get_account(&fixture.mint).unwrap();
    let mint = Mint::unpack(&mint_account.data).unwrap();
    assert_eq!(mint.decimals, 9);
    assert_eq!(mint.supply, FULL_TOTAL_SUPPLY);
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(mint.freeze_authority, COption::None);

    let beneficiary = read_vault(&fixture, fixture.beneficiary_vault);
    let purpose = read_vault(&fixture, fixture.purpose_vault);
    assert_eq!(beneficiary.deposited_amount, FULL_BENEFICIARY_DEPOSIT);
    assert_eq!(purpose.deposited_amount, FULL_PURPOSE_DEPOSIT);
    assert_eq!(beneficiary.monthly_cap, amounts.beneficiary_cap);
    assert_eq!(purpose.monthly_cap, amounts.purpose_cap);
    assert_eq!(token_balance(&fixture.svm, fixture.depositor_token), 0);
    assert_eq!(
        token_balance(&fixture.svm, fixture.beneficiary_vault_token),
        FULL_BENEFICIARY_DEPOSIT
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.purpose_vault_token),
        FULL_PURPOSE_DEPOSIT
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.genesis_token),
        FULL_GENESIS_ALLOCATION
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.lp_token),
        FULL_LP_ALLOCATION
    );
    assert_eq!(read_market(&fixture).eligible_volume, FULL_ELIGIBLE_VOLUME);
    assert_eq!(read_policy(&fixture).vault_count, 2);

    let reconciled = token_balance(&fixture.svm, fixture.beneficiary_vault_token)
        + token_balance(&fixture.svm, fixture.purpose_vault_token)
        + token_balance(&fixture.svm, fixture.genesis_token)
        + token_balance(&fixture.svm, fixture.lp_token);
    assert_eq!(reconciled, mint.supply);

    let period = first_joint_period();
    let destination = fixture.contractor_token;
    let cliff_end = beneficiary.cliff_end_ts;
    approve_and_advance(
        cliff_end,
        period,
        amounts.purpose_cap,
        destination,
        &mut fixture,
    );
    refresh_market(&mut fixture);

    send(
        release_beneficiary_instruction(&fixture, amounts.beneficiary_cap),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();

    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, amounts.purpose_cap),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AggregateCapacityExceeded");

    let headroom = amounts.market_capacity - amounts.beneficiary_cap;
    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, headroom + 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AggregateCapacityExceeded");

    send(
        release_purpose_instruction(&fixture, destination, period, headroom),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(token_balance(&fixture.svm, destination), headroom);
    assert_eq!(
        read_policy(&fixture).released_this_period,
        amounts.market_capacity
    );
}

#[test]
fn r3_b_transaction_created_full_scale_graph_reconciles_and_releases() {
    let (mut fixture, treasury_token) = setup_transaction_created_full_scale();
    let amounts = fixture.amounts;

    let mint_account = fixture.svm.get_account(&fixture.mint).unwrap();
    assert_eq!(mint_account.owner, TOKEN_PROGRAM_ID);
    assert_eq!(mint_account.data.len(), Mint::LEN);
    let mint = Mint::unpack(&mint_account.data).unwrap();
    assert_eq!(mint.supply, FULL_TOTAL_SUPPLY);
    assert_eq!(mint.decimals, amounts.decimals);
    assert_eq!(mint.mint_authority, COption::None);
    assert_eq!(mint.freeze_authority, COption::None);

    // Both transaction-created staging accounts are emptied into B2; the two
    // untouched allocations plus both B2 vault accounts still conserve the
    // exact supply under raw account-byte decoding.
    assert_eq!(token_balance(&fixture.svm, fixture.depositor_token), 0);
    assert_eq!(token_balance(&fixture.svm, treasury_token), 0);
    let balances = [
        token_balance(&fixture.svm, fixture.beneficiary_vault_token),
        token_balance(&fixture.svm, fixture.purpose_vault_token),
        token_balance(&fixture.svm, fixture.genesis_token),
        token_balance(&fixture.svm, fixture.lp_token),
    ];
    assert_eq!(balances[0], amounts.beneficiary_deposit);
    assert_eq!(balances[1], amounts.purpose_deposit);
    assert_eq!(balances[2], amounts.genesis_allocation);
    assert_eq!(balances[3], amounts.lp_allocation);
    assert_eq!(balances.into_iter().sum::<u64>(), mint.supply);

    let beneficiary = read_vault(&fixture, fixture.beneficiary_vault);
    let period = first_joint_period();
    let destination = fixture.contractor_token;
    approve_and_advance(
        beneficiary.cliff_end_ts,
        period,
        amounts.purpose_cap,
        destination,
        &mut fixture,
    );
    refresh_market(&mut fixture);

    send(
        release_beneficiary_instruction(&fixture, amounts.beneficiary_cap),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    let headroom = amounts.market_capacity - amounts.beneficiary_cap;
    let too_much = send(
        release_purpose_instruction(&fixture, destination, period, headroom + 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(too_much, "AggregateCapacityExceeded");
    send(
        release_purpose_instruction(&fixture, destination, period, headroom),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(
        read_policy(&fixture).released_this_period,
        amounts.market_capacity
    );
    assert_eq!(token_balance(&fixture.svm, destination), headroom);
}

#[test]
fn an_approved_purpose_release_moves_tokens_and_debits_both_counters() {
    let mut fixture = setup();
    let period = 3;
    let destination = fixture.contractor_token;
    approve_and_advance(
        period_start(&fixture, 1),
        period,
        1_000_000,
        destination,
        &mut fixture,
    );
    refresh_market(&mut fixture);

    send(
        release_purpose_instruction(&fixture, destination, period, 600_000),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();

    assert_eq!(token_balance(&fixture.svm, destination), 600_000);
    assert_eq!(
        token_balance(&fixture.svm, fixture.purpose_vault_token),
        PURPOSE_DEPOSIT - 600_000
    );

    let vault = read_vault(&fixture, fixture.purpose_vault);
    assert_eq!(vault.released_this_period, 600_000);
    assert_eq!(vault.released_total, 600_000);
    assert_eq!(vault.current_period_index, period);

    let policy = read_policy(&fixture);
    assert_eq!(policy.released_this_period, 600_000);
    assert_eq!(policy.current_period_index, period);

    let approval = read_approval(&fixture, fixture.purpose_vault, period);
    assert_eq!(approval.approved_need, 1_000_000);
    assert_eq!(approval.consumed, 600_000);
}

#[test]
fn purpose_release_without_an_approval_is_rejected() {
    let mut fixture = setup();
    set_time(period_start(&fixture, 2), &mut fixture);
    let outcome = send(
        release_purpose_instruction(&fixture, fixture.contractor_token, 2, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AccountNotInitialized");
    assert_eq!(token_balance(&fixture.svm, fixture.contractor_token), 0);
}

#[test]
fn purpose_release_above_the_approved_need_is_rejected() {
    let mut fixture = setup();
    let period = 3;
    let destination = fixture.contractor_token;
    approve_and_advance(
        period_start(&fixture, 1),
        period,
        1_000_000,
        destination,
        &mut fixture,
    );

    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, 1_000_001),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "ApprovedNeedExceeded");
    assert_eq!(token_balance(&fixture.svm, destination), 0);
}

#[test]
fn purpose_release_above_the_vault_rate_cap_is_rejected() {
    let mut fixture = setup();
    let period = 3;
    let destination = fixture.contractor_token;
    // Approve more than the vault's own monthly cap; the cap must still bind.
    approve_and_advance(
        period_start(&fixture, 1),
        period,
        PURPOSE_CAP + 1,
        destination,
        &mut fixture,
    );
    refresh_market(&mut fixture);

    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, PURPOSE_CAP + 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "PeriodCapExceeded");
    assert_eq!(token_balance(&fixture.svm, destination), 0);
}

#[test]
fn two_vaults_each_within_cap_cannot_jointly_exceed_market_capacity() {
    let mut fixture = setup();
    let period = first_joint_period();
    let destination = fixture.contractor_token;
    let cliff_end = read_vault(&fixture, fixture.beneficiary_vault).cliff_end_ts;
    approve_and_advance(cliff_end, period, PURPOSE_CAP, destination, &mut fixture);
    refresh_market(&mut fixture);

    // The beneficiary takes its entire monthly cap first.
    send(
        release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(read_policy(&fixture).released_this_period, BENEFICIARY_CAP);

    // The purpose vault is individually entitled to PURPOSE_CAP, and every
    // other gate passes. Only the shared ceiling stops it.
    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, PURPOSE_CAP),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AggregateCapacityExceeded");
    assert_eq!(token_balance(&fixture.svm, destination), 0);

    // Exactly the remaining headroom is allowed, and not one unit more.
    let headroom = MARKET_CAPACITY - BENEFICIARY_CAP;
    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, headroom + 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AggregateCapacityExceeded");

    send(
        release_purpose_instruction(&fixture, destination, period, headroom),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(token_balance(&fixture.svm, destination), headroom);
    assert_eq!(read_policy(&fixture).released_this_period, MARKET_CAPACITY);
}

#[test]
fn zero_eligible_volume_rejects_every_release() {
    let mut fixture = setup();
    let period = 3;
    let destination = fixture.contractor_token;
    approve_and_advance(
        period_start(&fixture, 1),
        period,
        1_000_000,
        destination,
        &mut fixture,
    );

    let oracle = fixture.oracle.insecure_clone();
    send(
        report_instruction(oracle.pubkey(), 0),
        &[&oracle],
        &mut fixture.svm,
    )
    .unwrap();

    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AggregateCapacityExceeded");
    assert_eq!(token_balance(&fixture.svm, destination), 0);
}

#[test]
fn a_stale_market_input_rejects_instead_of_reusing_the_last_value() {
    let mut fixture = setup();
    let period = 3;
    let destination = fixture.contractor_token;
    approve_and_advance(
        period_start(&fixture, 1),
        period,
        1_000_000,
        destination,
        &mut fixture,
    );

    // Refresh at the start of the period so the input is unambiguously fresh,
    // then let exactly one second past the tolerance elapse.
    let oracle = fixture.oracle.insecure_clone();
    send(
        report_instruction(oracle.pubkey(), ELIGIBLE_VOLUME),
        &[&oracle],
        &mut fixture.svm,
    )
    .unwrap();
    let reported_at = read_market(&fixture).updated_at;

    set_time(reported_at + MAX_AGE_SECONDS, &mut fixture);
    send(
        release_purpose_instruction(&fixture, destination, period, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();

    set_time(reported_at + MAX_AGE_SECONDS + 1, &mut fixture);
    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "StaleMarketInput");

    // The last reported volume is still sitting in the account. A fallback
    // implementation would have released here; this one must not.
    assert_eq!(read_market(&fixture).eligible_volume, ELIGIBLE_VOLUME);
    assert_eq!(token_balance(&fixture.svm, destination), 1);
}

#[test]
fn a_policy_whose_oracle_never_reported_releases_nothing() {
    let mut fixture = setup_with(PolicyConfig {
        report_volume: false,
        ..PolicyConfig::default()
    });
    assert_eq!(read_market(&fixture).updated_at, 0);

    let period = 3;
    let destination = fixture.contractor_token;
    approve_and_advance(
        period_start(&fixture, 1),
        period,
        1_000_000,
        destination,
        &mut fixture,
    );

    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "StaleMarketInput");
}

#[test]
fn an_approver_may_not_approve_a_destination_it_owns() {
    let mut fixture = setup();
    set_time(period_start(&fixture, 1), &mut fixture);
    let approver = fixture.approver.insecure_clone();
    let outcome = send(
        approve_instruction(
            approver.pubkey(),
            fixture.mint,
            fixture.purpose_vault,
            fixture.approver_token,
            3,
            1_000_000,
        ),
        &[&approver],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "ApproverIsPayee");
}

#[test]
fn an_approval_for_the_current_period_is_rejected() {
    let mut fixture = setup();
    set_time(period_start(&fixture, 2), &mut fixture);
    let approver = fixture.approver.insecure_clone();
    let outcome = send(
        approve_instruction(
            approver.pubkey(),
            fixture.mint,
            fixture.purpose_vault,
            fixture.contractor_token,
            2,
            1_000_000,
        ),
        &[&approver],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "ApprovalPeriodTooSoon");
}

#[test]
fn an_approval_younger_than_the_notice_period_cannot_be_consumed() {
    let mut fixture = setup();
    let destination = fixture.contractor_token;
    // Written near the very end of period 0, consumed at the start of period 1:
    // the period index matches, but only a hundred seconds of notice elapsed.
    approve_and_advance(
        period_start(&fixture, 1) - 100,
        1,
        1_000_000,
        destination,
        &mut fixture,
    );

    let outcome = send(
        release_purpose_instruction(&fixture, destination, 1, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "NoticePeriodActive");
    assert_eq!(token_balance(&fixture.svm, destination), 0);
}

#[test]
fn an_approval_cannot_be_spent_in_a_different_period() {
    let mut fixture = setup();
    let destination = fixture.contractor_token;
    approve_and_advance(
        period_start(&fixture, 1),
        3,
        1_000_000,
        destination,
        &mut fixture,
    );

    set_time(period_start(&fixture, 4), &mut fixture);
    let outcome = send(
        release_purpose_instruction(&fixture, destination, 3, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "ApprovalPeriodMismatch");
    assert_eq!(token_balance(&fixture.svm, destination), 0);
}

#[test]
fn a_release_to_an_unapproved_destination_is_rejected() {
    let mut fixture = setup();
    let period = 3;
    approve_and_advance(
        period_start(&fixture, 1),
        period,
        1_000_000,
        fixture.contractor_token,
        &mut fixture,
    );

    let outcome = send(
        release_purpose_instruction(&fixture, fixture.beneficiary_token, period, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "ApprovalDestinationMismatch");
    assert_eq!(token_balance(&fixture.svm, fixture.beneficiary_token), 0);
}

#[test]
fn beneficiary_release_before_the_cliff_is_rejected() {
    let mut fixture = setup();
    set_time(period_start(&fixture, 3), &mut fixture);
    let outcome = send(
        release_beneficiary_instruction(&fixture, 1),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "CliffActive");
    assert_eq!(token_balance(&fixture.svm, fixture.beneficiary_token), 0);
}

#[test]
fn unused_capacity_expires_in_both_the_vault_and_the_shared_window() {
    let mut fixture = setup();
    let period = first_joint_period();
    let cliff_end = read_vault(&fixture, fixture.beneficiary_vault).cliff_end_ts;
    set_time(period_start(&fixture, period), &mut fixture);
    refresh_market(&mut fixture);

    send(
        release_beneficiary_instruction(&fixture, 1),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(read_policy(&fixture).released_this_period, 1);

    // A later period must not inherit the unused BENEFICIARY_CAP - 1.
    set_time(period_start(&fixture, period + 1), &mut fixture);
    refresh_market(&mut fixture);
    let outcome = send(
        release_beneficiary_instruction(&fixture, BENEFICIARY_CAP + 1),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "PeriodCapExceeded");

    send(
        release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();

    let vault = read_vault(&fixture, fixture.beneficiary_vault);
    assert_eq!(vault.released_this_period, BENEFICIARY_CAP);
    assert_eq!(vault.released_total, BENEFICIARY_CAP + 1);
    let policy = read_policy(&fixture);
    assert_eq!(policy.released_this_period, BENEFICIARY_CAP);
    assert_eq!(policy.current_period_index, period + 1);
    assert!(cliff_end < period_start(&fixture, period));
}

#[test]
fn a_stranger_cannot_attach_a_vault_to_someone_elses_capacity_window() {
    let mut fixture = setup();
    let stranger = Keypair::new();
    fixture
        .svm
        .airdrop(&stranger.pubkey(), 10_000_000_000)
        .unwrap();
    let stranger_source = Pubkey::new_unique();
    fixture
        .svm
        .set_account(
            stranger_source,
            token_account(fixture.mint, stranger.pubkey(), 1_000_000_000),
        )
        .unwrap();

    let (instruction, vault, vault_token) = deposit_instruction(
        stranger.pubkey(),
        stranger.pubkey(),
        stranger.pubkey(),
        fixture.mint,
        stranger_source,
        DepositArgs {
            kind: VaultKind::Purpose,
            amount: 1_000_000_000,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: 0,
        },
    );
    let outcome = send(instruction, &[&stranger], &mut fixture.svm);
    assert_failed_with(outcome, "WrongPolicyAuthority");
    assert!(fixture.svm.get_account(&vault).is_none());
    assert!(fixture.svm.get_account(&vault_token).is_none());
    assert_eq!(read_policy(&fixture).vault_count, 2);
}

#[test]
fn a_purpose_vault_may_not_be_given_a_cliff() {
    let mut fixture = setup();
    let other_authority = Keypair::new();
    let source = Pubkey::new_unique();
    fixture
        .svm
        .set_account(
            source,
            token_account(fixture.mint, fixture.depositor.pubkey(), 1_000_000_000),
        )
        .unwrap();

    let (instruction, vault, _) = deposit_instruction(
        fixture.depositor.pubkey(),
        fixture.policy_authority.pubkey(),
        other_authority.pubkey(),
        fixture.mint,
        source,
        DepositArgs {
            kind: VaultKind::Purpose,
            amount: 1_000_000_000,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: 1,
        },
    );
    let outcome = send(
        instruction,
        &[
            &fixture.depositor.insecure_clone(),
            &fixture.policy_authority.insecure_clone(),
        ],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "PurposeCliffNotZero");
    assert!(fixture.svm.get_account(&vault).is_none());
}

#[test]
fn a_beneficiary_vault_may_not_shorten_the_frozen_cliff() {
    let mut fixture = setup();
    let other_beneficiary = Keypair::new();
    let source = Pubkey::new_unique();
    fixture
        .svm
        .set_account(
            source,
            token_account(fixture.mint, fixture.depositor.pubkey(), 1_000_000_000),
        )
        .unwrap();

    let (instruction, vault, _) = deposit_instruction(
        fixture.depositor.pubkey(),
        fixture.policy_authority.pubkey(),
        other_beneficiary.pubkey(),
        fixture.mint,
        source,
        DepositArgs {
            kind: VaultKind::Beneficiary,
            amount: 1_000_000_000,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS - 1,
        },
    );
    let outcome = send(
        instruction,
        &[
            &fixture.depositor.insecure_clone(),
            &fixture.policy_authority.insecure_clone(),
        ],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "CliffTooShort");
    assert!(fixture.svm.get_account(&vault).is_none());
}

#[test]
fn only_the_frozen_oracle_may_report_volume() {
    let mut fixture = setup();
    let impostor = Keypair::new();
    fixture
        .svm
        .airdrop(&impostor.pubkey(), 10_000_000_000)
        .unwrap();
    let outcome = send(
        report_instruction(impostor.pubkey(), u64::MAX),
        &[&impostor],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "ConstraintHasOne");
    assert_eq!(read_market(&fixture).eligible_volume, ELIGIBLE_VOLUME);
}

#[test]
fn an_inflated_oracle_report_cannot_lift_a_release_past_the_frozen_schedule() {
    // What a captured oracle can do: widen the shared window until the
    // aggregate rule stops binding. What it cannot do: touch a vault's cap, its
    // cliff, its approved need or its notice period. This test spends the
    // widened window down to the last unit the frozen schedule allows, and then
    // shows the next unit is refused by a gate the oracle has no access to.
    let mut fixture = setup();
    let period = first_joint_period();
    let destination = fixture.contractor_token;
    let cliff_end = read_vault(&fixture, fixture.beneficiary_vault).cliff_end_ts;
    approve_and_advance(
        cliff_end,
        period,
        PURPOSE_CAP + 1,
        destination,
        &mut fixture,
    );
    report_volume(INFLATED_VOLUME, &mut fixture);

    // The window really did widen: this release would have been refused under
    // an honest report, since BENEFICIARY_CAP + PURPOSE_CAP > MARKET_CAPACITY.
    send(
        release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    send(
        release_purpose_instruction(&fixture, destination, period, PURPOSE_CAP),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(
        read_policy(&fixture).released_this_period,
        SUM_OF_MONTHLY_CAPS
    );

    // And there it stops. Both refusals name a per-vault gate, not the
    // aggregate one, which is the whole claim.
    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "PeriodCapExceeded");
    let outcome = send(
        release_beneficiary_instruction(&fixture, 1),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "PeriodCapExceeded");
    assert_eq!(token_balance(&fixture.svm, destination), PURPOSE_CAP);
}

#[test]
fn a_frozen_hard_ceiling_binds_the_window_below_the_market_term() {
    // The ceiling is the one term in the window no key can move: not the
    // oracle's, not the policy authority's. Here the market would allow
    // MARKET_CAPACITY and the ceiling allows less, so the ceiling decides.
    let mut fixture = setup_with(PolicyConfig {
        hard_ceiling: LOW_CEILING,
        ..PolicyConfig::default()
    });
    assert_eq!(read_policy(&fixture).hard_ceiling, LOW_CEILING);

    let period = first_joint_period();
    let destination = fixture.contractor_token;
    let cliff_end = read_vault(&fixture, fixture.beneficiary_vault).cliff_end_ts;
    approve_and_advance(cliff_end, period, PURPOSE_CAP, destination, &mut fixture);
    // An inflated report cannot buy back what the ceiling took away.
    report_volume(INFLATED_VOLUME, &mut fixture);

    send(
        release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();

    let headroom = LOW_CEILING - BENEFICIARY_CAP;
    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, headroom + 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AggregateCapacityExceeded");

    send(
        release_purpose_instruction(&fixture, destination, period, headroom),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(token_balance(&fixture.svm, destination), headroom);
    assert_eq!(read_policy(&fixture).released_this_period, LOW_CEILING);
}

#[test]
fn a_zero_hard_ceiling_is_refused_at_policy_creation() {
    // Zero would open a policy that can never release anything, with no
    // instruction to undo it. A deployment that wants no ceiling says u64::MAX.
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(purpose_vault::ID, program_path())
        .unwrap();
    let authority = Keypair::new();
    let oracle = Keypair::new();
    svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

    let mint = Pubkey::new_unique();
    let mut mint_data = vec![0; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut mint_data,
    )
    .unwrap();
    svm.set_account(
        mint,
        Account {
            lamports: 10_000_000,
            data: mint_data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let outcome = send(
        open_policy_instruction(
            authority.pubkey(),
            oracle.pubkey(),
            mint,
            MARKET_BPS,
            MAX_AGE_SECONDS,
            PolicyConfig {
                hard_ceiling: 0,
                ..PolicyConfig::default()
            },
        ),
        &[&authority],
        &mut svm,
    );
    assert_failed_with(outcome, "ZeroHardCeiling");

    send(
        open_policy_instruction(
            authority.pubkey(),
            oracle.pubkey(),
            mint,
            MARKET_BPS,
            MAX_AGE_SECONDS,
            PolicyConfig::default(),
        ),
        &[&authority],
        &mut svm,
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Oracle rotation. The repair for a lost or silent reporter, and the only
// authority anyone holds over the market input.
// ---------------------------------------------------------------------------

#[test]
fn only_the_policy_authority_may_propose_an_oracle() {
    let mut fixture = setup();
    let impostor = Keypair::new();
    let replacement = Keypair::new();
    fixture
        .svm
        .airdrop(&impostor.pubkey(), 10_000_000_000)
        .unwrap();

    let outcome = send(
        propose_oracle_instruction(impostor.pubkey(), replacement.pubkey()),
        &[&impostor],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "WrongPolicyAuthority");
    assert_eq!(read_market(&fixture).pending_oracle, Pubkey::default());

    // The oracle cannot promote itself either: reporting is its only power.
    let oracle = fixture.oracle.insecure_clone();
    let outcome = send(
        propose_oracle_instruction(oracle.pubkey(), replacement.pubkey()),
        &[&oracle],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "WrongPolicyAuthority");

    // And the default pubkey is not a proposal; it is the absence of one.
    let authority = fixture.policy_authority.insecure_clone();
    let outcome = send(
        propose_oracle_instruction(authority.pubkey(), Pubkey::default()),
        &[&authority],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "ZeroOracle");
}

#[test]
fn a_proposed_oracle_cannot_take_effect_before_its_ninety_day_notice() {
    let mut fixture = setup();
    let authority = fixture.policy_authority.insecure_clone();
    let replacement = Keypair::new();
    let proposed_at = fixture.genesis_ts + 60;
    set_time(proposed_at, &mut fixture);
    send(
        propose_oracle_instruction(authority.pubkey(), replacement.pubkey()),
        &[&authority],
        &mut fixture.svm,
    )
    .unwrap();

    let market = read_market(&fixture);
    assert_eq!(market.pending_oracle, replacement.pubkey());
    assert_eq!(market.pending_since, proposed_at);
    // The sitting oracle keeps its post for the whole notice.
    assert_eq!(market.oracle, fixture.oracle.pubkey());

    set_time(proposed_at + ROTATION_NOTICE - 1, &mut fixture);
    let outcome = send(
        execute_rotation_instruction(),
        &[&authority],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "RotationNoticeActive");
    assert_eq!(read_market(&fixture).oracle, fixture.oracle.pubkey());

    // Execution is permissionless once the notice is served: the authorisation
    // was the proposal, and the public already had its ninety days.
    set_time(proposed_at + ROTATION_NOTICE, &mut fixture);
    let stranger = Keypair::new();
    fixture
        .svm
        .airdrop(&stranger.pubkey(), 10_000_000_000)
        .unwrap();
    send(
        execute_rotation_instruction(),
        &[&stranger],
        &mut fixture.svm,
    )
    .unwrap();

    let market = read_market(&fixture);
    assert_eq!(market.oracle, replacement.pubkey());
    assert_eq!(market.pending_oracle, Pubkey::default());
    assert_eq!(market.pending_since, 0);
}

#[test]
fn executing_a_rotation_that_was_never_proposed_is_rejected() {
    let mut fixture = setup();
    let authority = fixture.policy_authority.insecure_clone();
    set_time(fixture.genesis_ts + 10 * ROTATION_NOTICE, &mut fixture);
    let outcome = send(
        execute_rotation_instruction(),
        &[&authority],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "NoPendingRotation");
}

#[test]
fn a_second_proposal_replaces_the_first_and_restarts_its_clock() {
    // This is also how a proposal is withdrawn. There is no cancel instruction:
    // proposing the oracle already in place lets the rotation that eventually
    // executes change nothing, which is a weaker power than a cancel and needs
    // no extra entry point.
    let mut fixture = setup();
    let authority = fixture.policy_authority.insecure_clone();
    let first = Keypair::new();
    let sitting = fixture.oracle.pubkey();

    let proposed_at = fixture.genesis_ts + 60;
    set_time(proposed_at, &mut fixture);
    send(
        propose_oracle_instruction(authority.pubkey(), first.pubkey()),
        &[&authority],
        &mut fixture.svm,
    )
    .unwrap();

    let withdrawn_at = proposed_at + ROTATION_NOTICE - 1;
    set_time(withdrawn_at, &mut fixture);
    send(
        propose_oracle_instruction(authority.pubkey(), sitting),
        &[&authority],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(read_market(&fixture).pending_since, withdrawn_at);

    // The first proposal's original deadline arrives and buys nothing.
    set_time(proposed_at + ROTATION_NOTICE, &mut fixture);
    let outcome = send(
        execute_rotation_instruction(),
        &[&authority],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "RotationNoticeActive");

    set_time(withdrawn_at + ROTATION_NOTICE, &mut fixture);
    send(
        execute_rotation_instruction(),
        &[&authority],
        &mut fixture.svm,
    )
    .unwrap();
    let market = read_market(&fixture);
    assert_eq!(market.oracle, sitting);
    assert_eq!(market.pending_oracle, Pubkey::default());
}

#[test]
fn a_rotation_restores_who_may_speak_and_nothing_else() {
    let mut fixture = setup();
    let authority = fixture.policy_authority.insecure_clone();
    let outgoing = fixture.oracle.insecure_clone();
    let incoming = Keypair::new();
    fixture
        .svm
        .airdrop(&incoming.pubkey(), 10_000_000_000)
        .unwrap();
    let before = read_market(&fixture);

    let proposed_at = fixture.genesis_ts + 60;
    set_time(proposed_at, &mut fixture);
    send(
        propose_oracle_instruction(authority.pubkey(), incoming.pubkey()),
        &[&authority],
        &mut fixture.svm,
    )
    .unwrap();
    set_time(proposed_at + ROTATION_NOTICE, &mut fixture);
    send(
        execute_rotation_instruction(),
        &[&authority],
        &mut fixture.svm,
    )
    .unwrap();

    // What the rotation did not do: it did not refresh the reported figure or
    // its timestamp. A stale input stays stale across the change, so a rotation
    // can never be used to reopen a window by itself.
    let after = read_market(&fixture);
    assert_eq!(after.eligible_volume, before.eligible_volume);
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.report_count, before.report_count);
    assert_eq!(after.market_capacity_bps, before.market_capacity_bps);
    assert_eq!(after.max_age_seconds, before.max_age_seconds);

    // What it did do: moved the post, in one direction only.
    let outcome = send(
        report_instruction(outgoing.pubkey(), u64::MAX),
        &[&outgoing],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "ConstraintHasOne");
    send(
        report_instruction(incoming.pubkey(), ELIGIBLE_VOLUME),
        &[&incoming],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(read_market(&fixture).report_count, before.report_count + 1);
}

#[test]
fn a_release_resumes_only_after_the_replacement_oracle_has_spoken() {
    // The whole recovery path, end to end: the reporter goes silent, the
    // authority proposes a replacement, the notice runs, the rotation executes,
    // and releases stay refused right up until the new reporter speaks.
    let mut fixture = setup();
    let authority = fixture.policy_authority.insecure_clone();
    let incoming = Keypair::new();
    fixture
        .svm
        .airdrop(&incoming.pubkey(), 10_000_000_000)
        .unwrap();

    let cliff_end = read_vault(&fixture, fixture.beneficiary_vault).cliff_end_ts;
    set_time(cliff_end, &mut fixture);
    send(
        propose_oracle_instruction(authority.pubkey(), incoming.pubkey()),
        &[&authority],
        &mut fixture.svm,
    )
    .unwrap();

    let rotated_at = cliff_end + ROTATION_NOTICE;
    set_time(rotated_at, &mut fixture);
    send(
        execute_rotation_instruction(),
        &[&authority],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(read_market(&fixture).oracle, incoming.pubkey());

    // Past the cliff, with a working oracle seat and no fresh report: refused.
    let outcome = send(
        release_beneficiary_instruction(&fixture, 1),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "StaleMarketInput");

    send(
        report_instruction(incoming.pubkey(), ELIGIBLE_VOLUME),
        &[&incoming],
        &mut fixture.svm,
    )
    .unwrap();
    send(
        release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(
        token_balance(&fixture.svm, fixture.beneficiary_token),
        BENEFICIARY_CAP
    );
    assert_eq!(
        read_policy(&fixture).current_period_index,
        period_at(&fixture, rotated_at)
    );
}

// ---------------------------------------------------------------------------
// The silence floor. The backstop for the case where the authority that would
// rotate the oracle is gone as well.
// ---------------------------------------------------------------------------

#[test]
fn without_a_declared_floor_a_silent_oracle_locks_both_vaults() {
    let mut fixture = setup();
    let period = first_joint_period();
    let destination = fixture.contractor_token;
    let cliff_end = read_vault(&fixture, fixture.beneficiary_vault).cliff_end_ts;
    // Deliberately no refresh_market: by the joint period the only report ever
    // made is years old.
    approve_and_advance(cliff_end, period, PURPOSE_CAP, destination, &mut fixture);

    let outcome = send(
        release_beneficiary_instruction(&fixture, 1),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "StaleMarketInput");
    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "StaleMarketInput");
    assert_eq!(token_balance(&fixture.svm, destination), 0);
}

#[test]
fn a_declared_floor_releases_a_trickle_and_only_after_the_grace_period() {
    let mut fixture = setup_with(PolicyConfig {
        silence_floor: SILENCE_FLOOR,
        silence_grace_seconds: SILENCE_GRACE,
        ..PolicyConfig::default()
    });
    let policy = read_policy(&fixture);
    assert_eq!(policy.silence_floor, SILENCE_FLOOR);
    assert_eq!(policy.silence_grace_seconds, SILENCE_GRACE);

    let destination = fixture.contractor_token;

    // Stale, but the silence is younger than the grace period, so the floor has
    // not engaged and the release is still refused. This is exactly the gap the
    // ninety-day rotation is meant to be completed inside.
    let early = 3;
    assert!(period_start(&fixture, early) - fixture.genesis_ts > MAX_AGE_SECONDS);
    assert!(period_start(&fixture, early) - fixture.genesis_ts < SILENCE_GRACE);
    approve_and_advance(
        fixture.genesis_ts,
        early,
        PURPOSE_CAP,
        destination,
        &mut fixture,
    );
    let outcome = send(
        release_purpose_instruction(&fixture, destination, early, 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "StaleMarketInput");

    // Past the grace period, and past the cliff so both vaults can compete.
    let period = first_joint_period();
    assert!(period_start(&fixture, period) - fixture.genesis_ts > SILENCE_GRACE);
    approve_and_advance(
        period_start(&fixture, early),
        period,
        PURPOSE_CAP,
        destination,
        &mut fixture,
    );

    // The floor is the whole shared window now, so the two vaults compete for
    // it exactly as they compete for the market term.
    let outcome = send(
        release_purpose_instruction(&fixture, destination, period, SILENCE_FLOOR + 1),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AggregateCapacityExceeded");

    send(
        release_purpose_instruction(&fixture, destination, period, SILENCE_FLOOR),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
    assert_eq!(token_balance(&fixture.svm, destination), SILENCE_FLOOR);

    let outcome = send(
        release_beneficiary_instruction(&fixture, 1),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    );
    assert_failed_with(outcome, "AggregateCapacityExceeded");

    // And an honest report puts the ordinary window back: the floor is a
    // backstop, not a ratchet.
    refresh_market(&mut fixture);
    send(
        release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    )
    .unwrap();
}

#[test]
fn a_policy_may_not_declare_half_a_silence_rule() {
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(purpose_vault::ID, program_path())
        .unwrap();
    let authority = Keypair::new();
    let oracle = Keypair::new();
    svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

    let mint = Pubkey::new_unique();
    let mut mint_data = vec![0; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut mint_data,
    )
    .unwrap();
    svm.set_account(
        mint,
        Account {
            lamports: 10_000_000,
            data: mint_data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let open = |config| {
        open_policy_instruction(
            authority.pubkey(),
            oracle.pubkey(),
            mint,
            MARKET_BPS,
            MAX_AGE_SECONDS,
            config,
        )
    };

    // A floor with no grace period would engage the instant an input went
    // stale, which is a fallback in everything but name.
    let outcome = send(
        open(PolicyConfig {
            silence_floor: SILENCE_FLOOR,
            silence_grace_seconds: 0,
            ..PolicyConfig::default()
        }),
        &[&authority],
        &mut svm,
    );
    assert_failed_with(outcome, "InvalidSilenceGrace");

    // Under the frozen minimum, and over the frozen maximum.
    for grace in [SILENCE_GRACE - 1, 730 * 24 * 60 * 60 + 1] {
        let outcome = send(
            open(PolicyConfig {
                silence_floor: SILENCE_FLOOR,
                silence_grace_seconds: grace,
                ..PolicyConfig::default()
            }),
            &[&authority],
            &mut svm,
        );
        assert_failed_with(outcome, "InvalidSilenceGrace");
    }

    // A grace period with no floor describes nothing.
    let outcome = send(
        open(PolicyConfig {
            silence_floor: 0,
            silence_grace_seconds: SILENCE_GRACE,
            ..PolicyConfig::default()
        }),
        &[&authority],
        &mut svm,
    );
    assert_failed_with(outcome, "InvalidSilenceGrace");

    send(open(PolicyConfig::default()), &[&authority], &mut svm).unwrap();
}

fn setup_with_freeze_authority(freeze_authority: Pubkey) -> Fixture {
    setup_amounts_with_freeze(
        PolicyConfig::default(),
        SCALED_AMOUNTS,
        COption::Some(freeze_authority),
    )
}

fn freeze_token(svm: &mut LiteSVM, token: Pubkey, mint: Pubkey, freezer: &Keypair) {
    let ix =
        token_instruction::freeze_account(&TOKEN_PROGRAM_ID, &token, &mint, &freezer.pubkey(), &[])
            .unwrap();
    send(ix, &[freezer], svm).expect("freeze_account must succeed while freeze authority is live");
    let data = svm.get_account(&token).unwrap().data;
    assert_eq!(
        SplAccount::unpack(&data).unwrap().state,
        AccountState::Frozen
    );
}

fn logs_show_frozen(
    outcome: Result<
        litesvm::types::TransactionMetadata,
        Box<litesvm::types::FailedTransactionMetadata>,
    >,
) {
    let failure = outcome.expect_err("expected the frozen vault token to reject the transfer");
    assert!(
        failure.meta.logs.iter().any(|line| {
            line.contains("AccountFrozen")
                || line.contains("Account is frozen")
                || line.contains("0x11")
        }),
        "expected an SPL frozen-account rejection, got:\n{}",
        failure.meta.logs.join("\n")
    );
}

#[test]
fn probe_a_b2_policy_squatting_hijacks_authority_and_permanently_blocks_creator() {
    // K4V-01: policy and market PDAs are [seed, policy_hash] only. A published
    // digest can be opened by a stranger, who then freezes authority, oracle
    // and ceiling. The intended operator cannot reopen or attach a vault.
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(purpose_vault::ID, program_path())
        .unwrap();

    let attacker = Keypair::new();
    let intended_authority = Keypair::new();
    let hostile_oracle = Keypair::new();
    let intended_oracle = Keypair::new();
    let depositor = Keypair::new();
    let beneficiary = Keypair::new();
    for key in [
        &attacker,
        &intended_authority,
        &hostile_oracle,
        &intended_oracle,
        &depositor,
        &beneficiary,
    ] {
        svm.airdrop(&key.pubkey(), 10_000_000_000).unwrap();
    }

    let mint = Pubkey::new_unique();
    let mut mint_data = vec![0; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority: COption::None,
            supply: BENEFICIARY_DEPOSIT,
            decimals: 9,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut mint_data,
    )
    .unwrap();
    svm.set_account(
        mint,
        Account {
            lamports: 10_000_000,
            data: mint_data,
            owner: TOKEN_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    let depositor_token = Pubkey::new_unique();
    svm.set_account(
        depositor_token,
        token_account(mint, depositor.pubkey(), BENEFICIARY_DEPOSIT),
    )
    .unwrap();

    let hostile = PolicyConfig {
        hard_ceiling: 1,
        ..PolicyConfig::default()
    };
    send(
        open_policy_instruction(
            attacker.pubkey(),
            hostile_oracle.pubkey(),
            mint,
            1,
            MAX_AGE_SECONDS,
            hostile,
        ),
        &[&attacker],
        &mut svm,
    )
    .expect("a stranger may open the published digest first");

    let policy =
        PolicyWindow::try_deserialize(&mut svm.get_account(&policy_pda()).unwrap().data.as_slice())
            .unwrap();
    let market =
        MarketInput::try_deserialize(&mut svm.get_account(&market_pda()).unwrap().data.as_slice())
            .unwrap();
    assert_eq!(policy.authority, attacker.pubkey());
    assert_eq!(policy.policy_hash, POLICY_HASH);
    assert_eq!(policy.hard_ceiling, 1);
    assert_eq!(market.oracle, hostile_oracle.pubkey());
    assert_eq!(market.market_capacity_bps, 1);

    let intended_open = send(
        open_policy_instruction(
            intended_authority.pubkey(),
            intended_oracle.pubkey(),
            mint,
            MARKET_BPS,
            MAX_AGE_SECONDS,
            PolicyConfig::default(),
        ),
        &[&intended_authority],
        &mut svm,
    )
    .expect_err("the canonical policy PDA cannot be reopened");
    assert!(
        intended_open
            .meta
            .logs
            .iter()
            .any(|line| line.contains("already in use")),
        "expected already-in-use, got:\n{}",
        intended_open.meta.logs.join("\n")
    );

    let (deposit_ix, vault, vault_token) = deposit_instruction(
        depositor.pubkey(),
        intended_authority.pubkey(),
        beneficiary.pubkey(),
        mint,
        depositor_token,
        DepositArgs {
            kind: VaultKind::Beneficiary,
            amount: BENEFICIARY_DEPOSIT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
        },
    );
    assert_failed_with(
        send(deposit_ix, &[&depositor, &intended_authority], &mut svm),
        "WrongPolicyAuthority",
    );
    assert!(svm.get_account(&vault).is_none());
    assert!(svm.get_account(&vault_token).is_none());
    assert_eq!(
        PolicyWindow::try_deserialize(&mut svm.get_account(&policy_pda()).unwrap().data.as_slice())
            .unwrap()
            .vault_count,
        0
    );

    // Capture is complete: the squatter can attach a vault, the publisher cannot.
    let (attacker_deposit, captured_vault, _) = deposit_instruction(
        depositor.pubkey(),
        attacker.pubkey(),
        beneficiary.pubkey(),
        mint,
        depositor_token,
        DepositArgs {
            kind: VaultKind::Beneficiary,
            amount: BENEFICIARY_DEPOSIT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
        },
    );
    send(attacker_deposit, &[&depositor, &attacker], &mut svm).unwrap();
    assert!(svm.get_account(&captured_vault).is_some());
}

#[test]
fn probe_b_b2_retained_freeze_authority_permanently_locks_both_vaults() {
    // K4V-03: open_policy and deposit accept a mint that still has a freeze
    // authority. After FreezeAccount, both release paths fail and B2 has no
    // thaw, close or migrate instruction.
    let freezer = Keypair::new();
    let mut fixture = setup_with_freeze_authority(freezer.pubkey());
    fixture
        .svm
        .airdrop(&freezer.pubkey(), 10_000_000_000)
        .unwrap();

    let period = first_joint_period();
    let destination = fixture.contractor_token;
    let cliff_end = read_vault(&fixture, fixture.beneficiary_vault).cliff_end_ts;
    approve_and_advance(cliff_end, period, PURPOSE_CAP, destination, &mut fixture);
    refresh_market(&mut fixture);

    freeze_token(
        &mut fixture.svm,
        fixture.beneficiary_vault_token,
        fixture.mint,
        &freezer,
    );
    freeze_token(
        &mut fixture.svm,
        fixture.purpose_vault_token,
        fixture.mint,
        &freezer,
    );

    let thaw_by_beneficiary = token_instruction::thaw_account(
        &TOKEN_PROGRAM_ID,
        &fixture.beneficiary_vault_token,
        &fixture.mint,
        &fixture.beneficiary.pubkey(),
        &[],
    )
    .unwrap();
    assert!(
        send(
            thaw_by_beneficiary,
            &[&fixture.beneficiary.insecure_clone()],
            &mut fixture.svm,
        )
        .is_err(),
        "only the freeze authority can thaw; the vault signer cannot"
    );

    logs_show_frozen(send(
        release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
        &[&fixture.beneficiary.insecure_clone()],
        &mut fixture.svm,
    ));
    logs_show_frozen(send(
        release_purpose_instruction(&fixture, destination, period, PURPOSE_CAP),
        &[&fixture.approver.insecure_clone()],
        &mut fixture.svm,
    ));

    assert_eq!(token_balance(&fixture.svm, fixture.beneficiary_token), 0);
    assert_eq!(token_balance(&fixture.svm, destination), 0);
    assert_eq!(
        token_balance(&fixture.svm, fixture.beneficiary_vault_token),
        BENEFICIARY_DEPOSIT
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.purpose_vault_token),
        PURPOSE_DEPOSIT
    );
}

fn preapprove_purpose_periods(fixture: &mut Fixture, start_period: u64, count: u64) {
    let destination = fixture.contractor_token;
    let approver = fixture.approver.insecure_clone();
    // Write at the start of the previous period so each approval has a full
    // 30-day notice by the time its named period opens.
    set_time(
        period_start(fixture, start_period) - PERIOD_SECONDS,
        fixture,
    );
    for period in start_period..start_period + count {
        send(
            approve_instruction(
                approver.pubkey(),
                fixture.mint,
                fixture.purpose_vault,
                destination,
                period,
                PURPOSE_CAP,
            ),
            &[&approver],
            &mut fixture.svm,
        )
        .expect("a future-period approval must be recorded");
    }
}

#[test]
fn probe_c_purpose_first_multi_period_shared_window_starvation() {
    // K4V-04 complete starvation: when hard_ceiling equals the purpose cap,
    // a purpose-first co-tenant consumes the entire shared window every
    // period. Unused beneficiary capacity expires; there is no reservation.
    let mut fixture = setup_with(PolicyConfig {
        hard_ceiling: PURPOSE_CAP,
        ..PolicyConfig::default()
    });
    let start_period = first_joint_period();
    let destination = fixture.contractor_token;
    preapprove_purpose_periods(&mut fixture, start_period, 6);

    for period in start_period..start_period + 6 {
        set_time(period_start(&fixture, period), &mut fixture);
        refresh_market(&mut fixture);

        send(
            release_purpose_instruction(&fixture, destination, period, PURPOSE_CAP),
            &[&fixture.approver.insecure_clone()],
            &mut fixture.svm,
        )
        .expect("purpose-first must consume the whole ceiling");
        assert_eq!(read_policy(&fixture).released_this_period, PURPOSE_CAP);

        assert_failed_with(
            send(
                release_beneficiary_instruction(&fixture, 1),
                &[&fixture.beneficiary.insecure_clone()],
                &mut fixture.svm,
            ),
            "AggregateCapacityExceeded",
        );
        assert_failed_with(
            send(
                release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
                &[&fixture.beneficiary.insecure_clone()],
                &mut fixture.svm,
            ),
            "AggregateCapacityExceeded",
        );
    }

    assert_eq!(
        read_vault(&fixture, fixture.beneficiary_vault).released_total,
        0
    );
    assert_eq!(token_balance(&fixture.svm, fixture.beneficiary_token), 0);
    assert_eq!(
        read_vault(&fixture, fixture.purpose_vault).released_total,
        6 * PURPOSE_CAP
    );
    assert_eq!(token_balance(&fixture.svm, destination), 6 * PURPOSE_CAP);
}

#[test]
fn probe_c_devnet_parameters_shared_window_squeeze_starves_beneficiary() {
    // Published devnet numbers do not zero the beneficiary. Purpose-first
    // leaves DEVNET_SQUEEZE_HEADROOM of BENEFICIARY_CAP each period, so six
    // months transfer 2_500_002 instead of 7_500_000. Complete starvation is
    // the previous test, where hard_ceiling equals the purpose cap.
    let mut fixture = setup_with(PolicyConfig {
        hard_ceiling: DEVNET_HARD_CEILING,
        ..PolicyConfig::default()
    });
    let start_period = first_joint_period();
    let destination = fixture.contractor_token;
    preapprove_purpose_periods(&mut fixture, start_period, 6);

    for period in start_period..start_period + 6 {
        set_time(period_start(&fixture, period), &mut fixture);
        refresh_market(&mut fixture);

        send(
            release_purpose_instruction(&fixture, destination, period, PURPOSE_CAP),
            &[&fixture.approver.insecure_clone()],
            &mut fixture.svm,
        )
        .unwrap();

        assert_failed_with(
            send(
                release_beneficiary_instruction(&fixture, BENEFICIARY_CAP),
                &[&fixture.beneficiary.insecure_clone()],
                &mut fixture.svm,
            ),
            "AggregateCapacityExceeded",
        );
        assert_failed_with(
            send(
                release_beneficiary_instruction(&fixture, DEVNET_SQUEEZE_HEADROOM + 1),
                &[&fixture.beneficiary.insecure_clone()],
                &mut fixture.svm,
            ),
            "AggregateCapacityExceeded",
        );
        send(
            release_beneficiary_instruction(&fixture, DEVNET_SQUEEZE_HEADROOM),
            &[&fixture.beneficiary.insecure_clone()],
            &mut fixture.svm,
        )
        .unwrap();
    }

    assert_eq!(
        read_vault(&fixture, fixture.beneficiary_vault).released_total,
        6 * DEVNET_SQUEEZE_HEADROOM
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.beneficiary_token),
        6 * DEVNET_SQUEEZE_HEADROOM
    );
    assert_eq!(
        read_vault(&fixture, fixture.purpose_vault).released_total,
        6 * PURPOSE_CAP
    );
}
