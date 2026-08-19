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
        APPROVAL_SEED, MARKET_SEED, MIN_CLIFF_SECONDS, PERIOD_SECONDS, POLICY_SEED,
        TOKEN_VAULT_SEED, VAULT_SEED,
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
use solana_transaction::Transaction;
use spl_token_interface::{
    state::{Account as SplAccount, AccountState, Mint},
    ID as TOKEN_PROGRAM_ID,
};
use std::path::PathBuf;

const BENEFICIARY_DEPOSIT: u64 = 300_000_000;
const PURPOSE_DEPOSIT: u64 = 500_000_000;
const ANNUAL_BPS: u16 = 500;
const BENEFICIARY_CAP: u64 = 1_250_000;
const PURPOSE_CAP: u64 = 2_083_333;
const MARKET_BPS: u16 = 250;
const ELIGIBLE_VOLUME: u64 = 120_000_000;
const MARKET_CAPACITY: u64 = 3_000_000;
const MAX_AGE_SECONDS: i64 = 3 * 24 * 60 * 60;
const POLICY_HASH: [u8; 32] = [0x42; 32];

/// The premise of the headline test, checked at compile time: each vault fits
/// under the market ceiling on its own, and the two together do not.
const _: () = assert!(BENEFICIARY_CAP <= MARKET_CAPACITY);
const _: () = assert!(PURPOSE_CAP <= MARKET_CAPACITY);
const _: () = assert!(BENEFICIARY_CAP + PURPOSE_CAP > MARKET_CAPACITY);

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
    policy: Pubkey,
    market: Pubkey,
    beneficiary_vault: Pubkey,
    beneficiary_vault_token: Pubkey,
    purpose_vault: Pubkey,
    purpose_vault_token: Pubkey,
    genesis_ts: i64,
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
    svm.expire_blockhash();
    let payer = signers[0];
    svm.send_transaction(Transaction::new_signed_with_payer(
        &[instruction],
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

fn open_policy_instruction(
    authority: Pubkey,
    oracle: Pubkey,
    mint: Pubkey,
    market_capacity_bps: u16,
    max_age_seconds: i64,
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
        }
        .data(),
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
fn first_joint_period() -> u64 {
    let cliff_period = (MIN_CLIFF_SECONDS / PERIOD_SECONDS) as u64;
    cliff_period + 2
}

fn setup_with(report_volume: bool) -> Fixture {
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
        supply: BENEFICIARY_DEPOSIT + PURPOSE_DEPOSIT,
        decimals: 9,
        is_initialized: true,
        freeze_authority: COption::None,
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
    svm.set_account(
        depositor_token,
        token_account(
            mint,
            depositor.pubkey(),
            BENEFICIARY_DEPOSIT + PURPOSE_DEPOSIT,
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

    send(
        open_policy_instruction(
            policy_authority.pubkey(),
            oracle.pubkey(),
            mint,
            MARKET_BPS,
            MAX_AGE_SECONDS,
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
            amount: BENEFICIARY_DEPOSIT,
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
            amount: PURPOSE_DEPOSIT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: 0,
        },
    );
    send(purpose_deposit, &[&depositor, &policy_authority], &mut svm).unwrap();

    if report_volume {
        send(
            report_instruction(oracle.pubkey(), ELIGIBLE_VOLUME),
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
        policy,
        market: market_pda(),
        beneficiary_vault,
        beneficiary_vault_token,
        purpose_vault,
        purpose_vault_token,
        genesis_ts,
    }
}

fn setup() -> Fixture {
    setup_with(true)
}

/// Approve at `approve_at`, then move to the start of `period_index`.
/// The oracle must speak again after any jump longer than the tolerance. That
/// is what the tolerance is for, so a test that moves a whole period forward
/// has to refresh rather than assume the old number still counts.
fn refresh_market(fixture: &mut Fixture) {
    let oracle = fixture.oracle.insecure_clone();
    send(
        report_instruction(oracle.pubkey(), ELIGIBLE_VOLUME),
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
    let mut fixture = setup_with(false);
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
