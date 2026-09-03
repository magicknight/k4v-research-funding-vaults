use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use beneficiary_vault::{
    accounts, constants::MIN_CLIFF_SECONDS, constants::PERIOD_SECONDS, instruction,
    state::BeneficiaryVault,
};
use litesvm::LiteSVM;
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

const DEPOSIT: u64 = 1_000_000_000;
const ANNUAL_BPS: u16 = 500;
const MONTHLY_CAP: u64 = 4_166_666;
const POLICY_HASH: [u8; 32] = [0x42; 32];

struct Fixture {
    svm: LiteSVM,
    depositor: Keypair,
    beneficiary: Keypair,
    mint: Pubkey,
    depositor_token: Pubkey,
    beneficiary_token: Pubkey,
    vault_state: Pubkey,
    vault_token: Pubkey,
}

struct DepositArgs {
    amount: u64,
    annual_release_bps: u16,
    cliff_seconds: i64,
    policy_hash: [u8; 32],
}

fn program_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy/beneficiary_vault.so")
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

fn send(
    svm: &mut LiteSVM,
    instruction: Instruction,
    payer: &Keypair,
) -> Result<litesvm::types::TransactionMetadata, Box<litesvm::types::FailedTransactionMetadata>> {
    svm.expire_blockhash();
    svm.send_transaction(Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[payer],
        svm.latest_blockhash(),
    ))
    .map_err(Box::new)
}

fn release_instruction(fixture: &Fixture, amount: u64) -> Instruction {
    Instruction {
        program_id: beneficiary_vault::ID,
        accounts: accounts::Release {
            beneficiary: fixture.beneficiary.pubkey(),
            mint: fixture.mint,
            vault_state: fixture.vault_state,
            vault_token: fixture.vault_token,
            beneficiary_token: fixture.beneficiary_token,
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::Release { amount }.data(),
    }
}

fn deposit_instruction(
    depositor: Pubkey,
    beneficiary: Pubkey,
    mint: Pubkey,
    depositor_token: Pubkey,
    args: DepositArgs,
) -> (Instruction, Pubkey, Pubkey) {
    let (vault_state, _) = Pubkey::find_program_address(
        &[
            b"beneficiary-vault",
            beneficiary.as_ref(),
            mint.as_ref(),
            args.policy_hash.as_ref(),
        ],
        &beneficiary_vault::ID,
    );
    let (vault_token, _) = Pubkey::find_program_address(
        &[b"beneficiary-token", vault_state.as_ref()],
        &beneficiary_vault::ID,
    );
    (
        Instruction {
            program_id: beneficiary_vault::ID,
            accounts: accounts::Deposit {
                depositor,
                beneficiary,
                mint,
                depositor_token,
                vault_state,
                vault_token,
                token_program: TOKEN_PROGRAM_ID,
                system_program: solana_system_interface::program::ID,
            }
            .to_account_metas(None),
            data: instruction::Deposit {
                amount: args.amount,
                annual_release_bps: args.annual_release_bps,
                cliff_seconds: args.cliff_seconds,
                policy_hash: args.policy_hash,
            }
            .data(),
        },
        vault_state,
        vault_token,
    )
}

fn read_state(fixture: &Fixture) -> BeneficiaryVault {
    let account = fixture.svm.get_account(&fixture.vault_state).unwrap();
    BeneficiaryVault::try_deserialize(&mut account.data.as_slice()).unwrap()
}

fn token_balance(svm: &LiteSVM, address: Pubkey) -> u64 {
    let account = svm.get_account(&address).unwrap();
    SplAccount::unpack(&account.data).unwrap().amount
}

fn setup() -> Fixture {
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(beneficiary_vault::ID, program_path())
        .unwrap();

    let depositor = Keypair::new();
    let beneficiary = Keypair::new();
    let mint = Pubkey::new_unique();
    let depositor_token = Pubkey::new_unique();
    let beneficiary_token = Pubkey::new_unique();
    svm.airdrop(&depositor.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&beneficiary.pubkey(), 10_000_000_000).unwrap();

    let mint_value = Mint {
        mint_authority: COption::Some(depositor.pubkey()),
        supply: DEPOSIT,
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
    svm.set_account(
        depositor_token,
        token_account(mint, depositor.pubkey(), DEPOSIT),
    )
    .unwrap();
    svm.set_account(
        beneficiary_token,
        token_account(mint, beneficiary.pubkey(), 0),
    )
    .unwrap();

    let (deposit_instruction, vault_state, vault_token) = deposit_instruction(
        depositor.pubkey(),
        beneficiary.pubkey(),
        mint,
        depositor_token,
        DepositArgs {
            amount: DEPOSIT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
            policy_hash: POLICY_HASH,
        },
    );
    send(&mut svm, deposit_instruction, &depositor).unwrap();

    Fixture {
        svm,
        depositor,
        beneficiary,
        mint,
        depositor_token,
        beneficiary_token,
        vault_state,
        vault_token,
    }
}

#[test]
fn deposit_creates_frozen_state_and_moves_all_tokens() {
    let fixture = setup();
    let state = read_state(&fixture);
    assert_eq!(state.depositor, fixture.depositor.pubkey());
    assert_eq!(state.beneficiary, fixture.beneficiary.pubkey());
    assert_eq!(state.mint, fixture.mint);
    assert_eq!(state.policy_hash, POLICY_HASH);
    assert_eq!(state.deposited_amount, DEPOSIT);
    assert_eq!(state.monthly_cap, MONTHLY_CAP);
    assert_eq!(state.cliff_end_ts - state.genesis_ts, MIN_CLIFF_SECONDS);
    assert_eq!(token_balance(&fixture.svm, fixture.depositor_token), 0);
    assert_eq!(token_balance(&fixture.svm, fixture.vault_token), DEPOSIT);
}

#[test]
fn cliff_and_period_cap_are_enforced_by_the_loaded_sbf_program() {
    let mut fixture = setup();

    let before_cliff = release_instruction(&fixture, 1);
    let failure = send(&mut fixture.svm, before_cliff, &fixture.beneficiary).unwrap_err();
    assert!(failure
        .meta
        .logs
        .iter()
        .any(|line| line.contains("CliffActive")));
    assert_eq!(token_balance(&fixture.svm, fixture.beneficiary_token), 0);

    let mut clock = fixture.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = read_state(&fixture).cliff_end_ts;
    fixture.svm.set_sysvar(&clock);
    let first_release = release_instruction(&fixture, MONTHLY_CAP);
    send(&mut fixture.svm, first_release, &fixture.beneficiary).unwrap();

    let over_cap = release_instruction(&fixture, 1);
    let failure = send(&mut fixture.svm, over_cap, &fixture.beneficiary).unwrap_err();
    assert!(failure
        .meta
        .logs
        .iter()
        .any(|line| line.contains("PeriodCapExceeded")));

    clock.unix_timestamp += PERIOD_SECONDS;
    fixture.svm.set_sysvar(&clock);
    let second_release = release_instruction(&fixture, MONTHLY_CAP);
    send(&mut fixture.svm, second_release, &fixture.beneficiary).unwrap();

    let state = read_state(&fixture);
    assert_eq!(state.current_period_index, 1);
    assert_eq!(state.released_this_period, MONTHLY_CAP);
    assert_eq!(state.released_total, 2 * MONTHLY_CAP);
    assert_eq!(
        token_balance(&fixture.svm, fixture.beneficiary_token),
        2 * MONTHLY_CAP
    );
    assert_eq!(
        token_balance(&fixture.svm, fixture.vault_token),
        DEPOSIT - 2 * MONTHLY_CAP
    );
}

#[test]
fn unused_capacity_expires_instead_of_carrying_forward() {
    let mut fixture = setup();
    let mut clock = fixture.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = read_state(&fixture).cliff_end_ts;
    fixture.svm.set_sysvar(&clock);

    let small_release = release_instruction(&fixture, 1);
    send(&mut fixture.svm, small_release, &fixture.beneficiary).unwrap();
    clock.unix_timestamp += PERIOD_SECONDS;
    fixture.svm.set_sysvar(&clock);

    let attempted_carry = release_instruction(&fixture, MONTHLY_CAP + 1);
    let failure = send(&mut fixture.svm, attempted_carry, &fixture.beneficiary).unwrap_err();
    assert!(failure
        .meta
        .logs
        .iter()
        .any(|line| line.contains("PeriodCapExceeded")));

    let fresh_cap = release_instruction(&fixture, MONTHLY_CAP);
    send(&mut fixture.svm, fresh_cap, &fixture.beneficiary).unwrap();
    let state = read_state(&fixture);
    assert_eq!(state.released_this_period, MONTHLY_CAP);
    assert_eq!(state.released_total, MONTHLY_CAP + 1);
}

#[test]
fn release_rejects_a_token_destination_not_owned_by_the_beneficiary() {
    let mut fixture = setup();
    let mut clock = fixture.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = read_state(&fixture).cliff_end_ts;
    fixture.svm.set_sysvar(&clock);

    let mut instruction = release_instruction(&fixture, 1);
    instruction.accounts[4].pubkey = fixture.depositor_token;
    let failure = send(&mut fixture.svm, instruction, &fixture.beneficiary).unwrap_err();
    assert!(failure
        .meta
        .logs
        .iter()
        .any(|line| line.contains("ConstraintTokenOwner")));
    assert_eq!(token_balance(&fixture.svm, fixture.depositor_token), 0);
}

#[test]
fn deposit_rejects_a_cliff_shorter_than_the_frozen_minimum() {
    let mut fixture = setup();
    let second_source = Pubkey::new_unique();
    fixture
        .svm
        .set_account(
            second_source,
            token_account(fixture.mint, fixture.depositor.pubkey(), DEPOSIT),
        )
        .unwrap();
    let (instruction, state, token) = deposit_instruction(
        fixture.depositor.pubkey(),
        fixture.beneficiary.pubkey(),
        fixture.mint,
        second_source,
        DepositArgs {
            amount: DEPOSIT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS - 1,
            policy_hash: [0x43; 32],
        },
    );

    let failure = send(&mut fixture.svm, instruction, &fixture.depositor).unwrap_err();
    assert!(failure
        .meta
        .logs
        .iter()
        .any(|line| line.contains("CliffTooShort")));
    assert!(fixture.svm.get_account(&state).is_none());
    assert!(fixture.svm.get_account(&token).is_none());
    assert_eq!(token_balance(&fixture.svm, second_source), DEPOSIT);
}

fn inject_mint(
    svm: &mut LiteSVM,
    mint: Pubkey,
    mint_authority: COption<Pubkey>,
    freeze_authority: COption<Pubkey>,
    supply: u64,
) {
    let mut mint_data = vec![0; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority,
            supply,
            decimals: 9,
            is_initialized: true,
            freeze_authority,
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
}

fn assert_already_in_use(failure: &litesvm::types::FailedTransactionMetadata) {
    assert!(
        failure
            .meta
            .logs
            .iter()
            .any(|line| line.contains("already in use")),
        "expected already-in-use, got:\n{}",
        failure.meta.logs.join("\n")
    );
}

fn assert_frozen_rejection(failure: &litesvm::types::FailedTransactionMetadata) {
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
fn probe_a_b1_dust_squatting_permanently_blocks_intended_deposit() {
    // K4V-02: vault PDA is [beneficiary, mint, policy_hash] with no depositor
    // seed. 240 base units at 500 bps is the smallest deposit that still
    // yields monthly_cap = 1, so a stranger can occupy the canonical address
    // for rent plus dust.
    const DUST_AMOUNT: u64 = 240;
    const INTENDED_AMOUNT: u64 = DEPOSIT;
    const _: () = assert!((DUST_AMOUNT * 500 / 10_000) / 12 == 1);
    const _: () = assert!((239u64 * 500 / 10_000) / 12 == 0);

    let mut svm = LiteSVM::new();
    svm.add_program_from_file(beneficiary_vault::ID, program_path())
        .unwrap();

    let attacker = Keypair::new();
    let intended_depositor = Keypair::new();
    let beneficiary = Keypair::new();
    let mint = Pubkey::new_unique();
    svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&intended_depositor.pubkey(), 10_000_000_000)
        .unwrap();
    svm.airdrop(&beneficiary.pubkey(), 10_000_000_000).unwrap();
    inject_mint(
        &mut svm,
        mint,
        COption::Some(intended_depositor.pubkey()),
        COption::None,
        DUST_AMOUNT + INTENDED_AMOUNT,
    );

    let attacker_token = Pubkey::new_unique();
    let intended_depositor_token = Pubkey::new_unique();
    svm.set_account(
        attacker_token,
        token_account(mint, attacker.pubkey(), DUST_AMOUNT),
    )
    .unwrap();
    svm.set_account(
        intended_depositor_token,
        token_account(mint, intended_depositor.pubkey(), INTENDED_AMOUNT),
    )
    .unwrap();

    let squatted_policy_hash = [0x77; 32];
    let (attacker_ix, vault_state, vault_token) = deposit_instruction(
        attacker.pubkey(),
        beneficiary.pubkey(),
        mint,
        attacker_token,
        DepositArgs {
            amount: DUST_AMOUNT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
            policy_hash: squatted_policy_hash,
        },
    );
    send(&mut svm, attacker_ix, &attacker).unwrap();

    let state = BeneficiaryVault::try_deserialize(
        &mut svm.get_account(&vault_state).unwrap().data.as_slice(),
    )
    .unwrap();
    assert_eq!(state.depositor, attacker.pubkey());
    assert_eq!(state.beneficiary, beneficiary.pubkey());
    assert_eq!(state.deposited_amount, DUST_AMOUNT);
    assert_eq!(state.monthly_cap, 1);
    assert_eq!(token_balance(&svm, vault_token), DUST_AMOUNT);

    let (intended_ix, intended_state, intended_token) = deposit_instruction(
        intended_depositor.pubkey(),
        beneficiary.pubkey(),
        mint,
        intended_depositor_token,
        DepositArgs {
            amount: INTENDED_AMOUNT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
            policy_hash: squatted_policy_hash,
        },
    );
    assert_eq!(intended_state, vault_state);
    assert_eq!(intended_token, vault_token);
    let failure = send(&mut svm, intended_ix, &intended_depositor)
        .expect_err("the occupied PDA must reject the intended deposit");
    assert_already_in_use(&failure);
    assert_eq!(
        token_balance(&svm, intended_depositor_token),
        INTENDED_AMOUNT
    );
    assert_eq!(token_balance(&svm, vault_token), DUST_AMOUNT);
}

#[test]
fn probe_b_b1_retained_freeze_authority_permanently_locks_vault() {
    // K4V-03: B1 deposit does not require freeze_authority == None. After
    // FreezeAccount the only remaining instruction, release, fails, and the
    // beneficiary cannot thaw.
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(beneficiary_vault::ID, program_path())
        .unwrap();

    let depositor = Keypair::new();
    let beneficiary = Keypair::new();
    let freezer = Keypair::new();
    let mint = Pubkey::new_unique();
    let depositor_token = Pubkey::new_unique();
    let beneficiary_token = Pubkey::new_unique();
    svm.airdrop(&depositor.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&beneficiary.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&freezer.pubkey(), 10_000_000_000).unwrap();
    inject_mint(
        &mut svm,
        mint,
        COption::Some(depositor.pubkey()),
        COption::Some(freezer.pubkey()),
        DEPOSIT,
    );
    svm.set_account(
        depositor_token,
        token_account(mint, depositor.pubkey(), DEPOSIT),
    )
    .unwrap();
    svm.set_account(
        beneficiary_token,
        token_account(mint, beneficiary.pubkey(), 0),
    )
    .unwrap();

    let (deposit_ix, vault_state, vault_token) = deposit_instruction(
        depositor.pubkey(),
        beneficiary.pubkey(),
        mint,
        depositor_token,
        DepositArgs {
            amount: DEPOSIT,
            annual_release_bps: ANNUAL_BPS,
            cliff_seconds: MIN_CLIFF_SECONDS,
            policy_hash: POLICY_HASH,
        },
    );
    send(&mut svm, deposit_ix, &depositor).unwrap();
    assert_eq!(token_balance(&svm, vault_token), DEPOSIT);

    let state = BeneficiaryVault::try_deserialize(
        &mut svm.get_account(&vault_state).unwrap().data.as_slice(),
    )
    .unwrap();
    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = state.cliff_end_ts;
    svm.set_sysvar(&clock);

    let freeze_ix = spl_token_interface::instruction::freeze_account(
        &TOKEN_PROGRAM_ID,
        &vault_token,
        &mint,
        &freezer.pubkey(),
        &[],
    )
    .unwrap();
    send(&mut svm, freeze_ix, &freezer).unwrap();
    assert_eq!(
        SplAccount::unpack(&svm.get_account(&vault_token).unwrap().data)
            .unwrap()
            .state,
        AccountState::Frozen
    );

    let thaw_ix = spl_token_interface::instruction::thaw_account(
        &TOKEN_PROGRAM_ID,
        &vault_token,
        &mint,
        &beneficiary.pubkey(),
        &[],
    )
    .unwrap();
    send(&mut svm, thaw_ix, &beneficiary).expect_err("the beneficiary is not the freeze authority");

    let release_ix = Instruction {
        program_id: beneficiary_vault::ID,
        accounts: accounts::Release {
            beneficiary: beneficiary.pubkey(),
            mint,
            vault_state,
            vault_token,
            beneficiary_token,
            token_program: TOKEN_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::Release {
            amount: MONTHLY_CAP,
        }
        .data(),
    };
    let release_err =
        send(&mut svm, release_ix, &beneficiary).expect_err("a frozen vault token cannot release");
    assert_frozen_rejection(&release_err);
    assert_eq!(token_balance(&svm, beneficiary_token), 0);
    assert_eq!(token_balance(&svm, vault_token), DEPOSIT);
}
