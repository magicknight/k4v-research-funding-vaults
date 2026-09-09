#![cfg(feature = "test-profile")]

use ::launch_vault_v4::{accounts, instruction, *};
use anchor_lang::{AccountDeserialize, AnchorSerialize, InstructionData, ToAccountMetas};
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
    ID as TOKEN_ID,
};
use std::path::PathBuf;

const START: i64 = 1_700_000_000;
const UNIT: u64 = 1_000_000_000;
const SUPPLY: u64 = 1_000_000_000 * UNIT;
type Outcome =
    Result<litesvm::types::TransactionMetadata, Box<litesvm::types::FailedTransactionMetadata>>;

fn ix(a: impl ToAccountMetas, d: impl InstructionData) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: a.to_account_metas(None),
        data: d.data(),
    }
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
        data,
        lamports: 10_000_000,
        owner: TOKEN_ID,
        executable: false,
        rent_epoch: 0,
    }
}

struct Fixture {
    svm: LiteSVM,
    creator: Keypair,
    founder: Keypair,
    treasury: Keypair,
    depositor: Keypair,
    oracle: Keypair,
    outsider: Keypair,
    recovery: [Keypair; 3],
    mint: Pubkey,
    source: Pubkey,
    founder_out: Pubkey,
    recipient: Pubkey,
    policy: Pubkey,
    config: LaunchConfig,
    hash: [u8; 32],
    last_signers: Vec<Pubkey>,
}

impl Fixture {
    fn new(disabled: bool, solo: bool) -> Self {
        let mut svm = LiteSVM::new();
        let directory = if disabled { "v4-disabled" } else { "v4-test" };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/{directory}/launch_vault_v4.so"));
        svm.add_program_from_file(ID, path).unwrap();
        let creator = Keypair::new();
        let same_or_new = || {
            if solo {
                Keypair::from_base58_string(&creator.to_base58_string())
            } else {
                Keypair::new()
            }
        };
        let founder = same_or_new();
        let treasury = same_or_new();
        let depositor = same_or_new();
        let oracle = Keypair::new();
        let outsider = Keypair::new();
        let recovery = [Keypair::new(), Keypair::new(), Keypair::new()];
        for key in [
            &creator, &founder, &treasury, &depositor, &oracle, &outsider,
        ] {
            svm.expire_blockhash();
            svm.airdrop(&key.pubkey(), 10_000_000_000).unwrap();
        }
        for k in &recovery {
            svm.airdrop(&k.pubkey(), 10_000_000_000).unwrap();
        }
        let mint = Pubkey::new_unique();
        let mut mint_data = vec![0; Mint::LEN];
        Mint::pack(
            Mint {
                mint_authority: COption::None,
                supply: SUPPLY,
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
                data: mint_data,
                lamports: 10_000_000,
                owner: TOKEN_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        let source = Pubkey::new_unique();
        let founder_out = Pubkey::new_unique();
        let recipient = Pubkey::new_unique();
        for (key, owner, amount) in [
            (source, depositor.pubkey(), SUPPLY),
            (founder_out, founder.pubkey(), 0),
            (recipient, outsider.pubkey(), 0),
        ] {
            svm.set_account(key, token_account(mint, owner, amount))
                .unwrap();
        }
        let config = LaunchConfig {
            t0: START + 2 * PERIOD,
            founder_amount: 300_000_000 * UNIT,
            treasury_amount: 500_000_000 * UNIT,
            founder_period_cap: 1_000_000 * UNIT,
            treasury_period_cap: 1_500_000 * UNIT,
            shared_hard_cap: 3_000_000 * UNIT,
            max_report_age: 86_400,
            recovery_keys: recovery.each_ref().map(|k| k.pubkey()),
            annual_rules: [
                AnnualRule {
                    start_period: 0,
                    end_period: 12,
                    founder_basis: 300_000_000 * UNIT,
                    treasury_basis: 500_000_000 * UNIT,
                    shared_cap: 40_000_000 * UNIT,
                    release_bps: 500,
                    source_hash: [70; 32],
                },
                AnnualRule {
                    start_period: 12,
                    end_period: 24,
                    founder_basis: 240_000_000 * UNIT,
                    treasury_basis: 480_000_000 * UNIT,
                    shared_cap: 36_000_000 * UNIT,
                    release_bps: 500,
                    source_hash: [71; 32],
                },
            ],
        };
        let hash = identity(
            &creator.pubkey(),
            &mint,
            &founder.pubkey(),
            &treasury.pubkey(),
            &oracle.pubkey(),
            &[42; 32],
            &config,
        );
        let policy = Pubkey::find_program_address(&[b"launch-v4-policy", &hash], &ID).0;
        let mut f = Self {
            svm,
            creator,
            founder,
            treasury,
            depositor,
            oracle,
            outsider,
            recovery,
            mint,
            source,
            founder_out,
            recipient,
            policy,
            config,
            hash,
            last_signers: vec![],
        };
        f.time(START);
        f
    }

    fn run(&mut self, instruction: Instruction) -> Outcome {
        self.run_many(vec![instruction], self.creator.pubkey())
    }

    fn run_many(&mut self, instructions: Vec<Instruction>, payer: Pubkey) -> Outcome {
        self.svm.expire_blockhash();
        let mut signers: Vec<&Keypair> = vec![];
        for k in [
            &self.creator,
            &self.founder,
            &self.treasury,
            &self.depositor,
            &self.oracle,
            &self.outsider,
            &self.recovery[0],
            &self.recovery[1],
            &self.recovery[2],
        ] {
            if (k.pubkey() == payer
                || instructions.iter().any(|ix| {
                    ix.accounts
                        .iter()
                        .any(|m| m.pubkey == k.pubkey() && m.is_signer)
                }))
                && !signers.iter().any(|s| s.pubkey() == k.pubkey())
            {
                signers.push(k);
            }
        }
        self.last_signers = signers.iter().map(|k| k.pubkey()).collect();
        self.svm
            .send_transaction(Transaction::new_signed_with_payer(
                &instructions,
                Some(&payer),
                &signers,
                self.svm.latest_blockhash(),
            ))
            .map_err(Box::new)
    }

    fn rebind(&mut self) {
        self.hash = identity(
            &self.creator.pubkey(),
            &self.mint,
            &self.founder.pubkey(),
            &self.treasury.pubkey(),
            &self.oracle.pubkey(),
            &[42; 32],
            &self.config,
        );
        self.policy = Pubkey::find_program_address(&[b"launch-v4-policy", &self.hash], &ID).0;
    }

    fn time(&mut self, now: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = now;
        self.svm.set_sysvar(&clock);
    }

    fn open_ix(&self) -> Instruction {
        ix(
            accounts::OpenPolicy {
                creator: self.creator.pubkey(),
                founder: self.founder.pubkey(),
                treasury: self.treasury.pubkey(),
                oracle: self.oracle.pubkey(),
                recovery_one: self.recovery[0].pubkey(),
                recovery_two: self.recovery[1].pubkey(),
                recovery_three: self.recovery[2].pubkey(),
                mint: self.mint,
                policy: self.policy,
                system_program: solana_system_interface::program::ID,
            },
            instruction::OpenPolicy {
                config: self.config,
                spec_hash: [42; 32],
                identity: self.hash,
            },
        )
    }

    fn vault(&self, role: u8) -> Pubkey {
        Pubkey::find_program_address(&[b"launch-v4-vault", self.policy.as_ref(), &[role]], &ID).0
    }

    fn vault_token(&self, role: u8) -> Pubkey {
        Pubkey::find_program_address(&[b"launch-v4-token", self.vault(role).as_ref()], &ID).0
    }

    fn approval(&self, period: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"launch-v4-approval",
                self.policy.as_ref(),
                &period.to_le_bytes(),
            ],
            &ID,
        )
        .0
    }

    fn deposit_ix(&self, role: u8, amount: u64) -> Instruction {
        ix(
            accounts::Deposit {
                creator: self.creator.pubkey(),
                depositor: self.depositor.pubkey(),
                authority: if role == FOUNDER {
                    self.founder.pubkey()
                } else {
                    self.treasury.pubkey()
                },
                policy: self.policy,
                mint: self.mint,
                source: self.source,
                vault: self.vault(role),
                vault_token: self.vault_token(role),
                token_program: TOKEN_ID,
                system_program: solana_system_interface::program::ID,
            },
            instruction::Deposit { role, amount },
        )
    }

    fn fund(&mut self) {
        self.run(self.open_ix()).unwrap();
        self.run(self.deposit_ix(FOUNDER, self.config.founder_amount))
            .unwrap();
        self.run(self.deposit_ix(TREASURY, self.config.treasury_amount))
            .unwrap();
    }

    fn arm_ix(&self) -> Instruction {
        ix(
            accounts::Control {
                creator: self.creator.pubkey(),
                policy: self.policy,
            },
            instruction::Arm {},
        )
    }

    fn activate_ix(&self) -> Instruction {
        ix(
            accounts::PolicyOnly {
                policy: self.policy,
            },
            instruction::Activate {},
        )
    }

    fn expire_ix(&self) -> Instruction {
        ix(
            accounts::PolicyOnly {
                policy: self.policy,
            },
            instruction::ExpireUnarmed {},
        )
    }

    fn cancel_ix(&self) -> Instruction {
        ix(
            accounts::ConsentControl {
                creator: self.creator.pubkey(),
                founder: self.founder.pubkey(),
                treasury: self.treasury.pubkey(),
                policy: self.policy,
            },
            instruction::Cancel {},
        )
    }

    fn refund_ix(&self, role: u8, destination: Pubkey) -> Instruction {
        ix(
            accounts::Refund {
                depositor: self.depositor.pubkey(),
                policy: self.policy,
                vault: self.vault(role),
                mint: self.mint,
                vault_token: self.vault_token(role),
                destination,
                token_program: TOKEN_ID,
            },
            instruction::Refund {},
        )
    }

    fn approve_ix(&self, period: u64, need: u64, recipient: Pubkey) -> Instruction {
        ix(
            accounts::ApproveTreasury {
                treasury: self.treasury.pubkey(),
                policy: self.policy,
                mint: self.mint,
                recipient,
                approval: self.approval(period),
                system_program: solana_system_interface::program::ID,
            },
            instruction::ApproveTreasury { period, need },
        )
    }

    fn report_ix(&self, capacity: u64, observed_at: i64, sequence: u64) -> Instruction {
        ix(
            accounts::Report {
                oracle: self.oracle.pubkey(),
                policy: self.policy,
            },
            instruction::ReportCapacity {
                capacity,
                observed_at,
                sequence,
                epoch: self.p().oracle_epoch,
            },
        )
    }

    fn report(&mut self, capacity: u64) {
        let now = self.svm.get_sysvar::<Clock>().unix_timestamp;
        self.run(self.report_ix(capacity, now, self.p().report_sequence + 1))
            .unwrap();
    }

    fn release_ix(&self, role: u8, amount: u64, approval_period: u64) -> Instruction {
        ix(
            accounts::Release {
                authority: if role == FOUNDER {
                    self.founder.pubkey()
                } else {
                    self.treasury.pubkey()
                },
                policy: self.policy,
                vault: self.vault(role),
                mint: self.mint,
                vault_token: self.vault_token(role),
                destination: if role == FOUNDER {
                    self.founder_out
                } else {
                    self.recipient
                },
                approval: if role == FOUNDER {
                    None
                } else {
                    Some(self.approval(approval_period))
                },
                token_program: TOKEN_ID,
            },
            instruction::Release { amount },
        )
    }

    fn active(&mut self) {
        self.fund();
        self.run(self.arm_ix()).unwrap();
        self.time(self.config.t0);
        self.run(self.activate_ix()).unwrap();
    }

    fn at_cliff(&mut self) {
        self.active();
        self.time(self.config.t0 + CLIFF);
        self.report(self.config.shared_hard_cap);
    }

    fn p(&self) -> LaunchPolicyV4 {
        self.read(self.policy)
    }
    fn v(&self, role: u8) -> LaunchVaultV4 {
        self.read(self.vault(role))
    }
    fn read<T: AccountDeserialize>(&self, address: Pubkey) -> T {
        T::try_deserialize(&mut self.svm.get_account(&address).unwrap().data.as_slice()).unwrap()
    }
    fn balance(&self, key: Pubkey) -> u64 {
        SplAccount::unpack(&self.svm.get_account(&key).unwrap().data)
            .unwrap()
            .amount
    }
    fn conserved(&self) {
        let total: u128 = [
            self.source,
            self.founder_out,
            self.recipient,
            self.vault_token(FOUNDER),
            self.vault_token(TREASURY),
        ]
        .iter()
        .filter(|key| self.svm.get_account(key).is_some())
        .map(|key| u128::from(self.balance(*key)))
        .sum();
        assert_eq!(total, u128::from(SUPPLY));
    }
    fn reject_unchanged(&mut self, instruction: Instruction, error: &str) {
        // Ignore payer SOL fees; every instruction account's data must roll back.
        let before: Vec<_> = instruction
            .accounts
            .iter()
            .map(|a| (a.pubkey, self.svm.get_account(&a.pubkey).map(|v| v.data)))
            .collect();
        rejected(self.run(instruction), error);
        for (key, data) in before {
            assert_eq!(
                self.svm.get_account(&key).map(|v| v.data),
                data,
                "account {key} changed on rejection"
            );
        }
    }
}

fn rejected(result: Outcome, needle: &str) {
    let failure = result.expect_err("transaction must reject");
    assert!(
        failure.meta.logs.iter().any(|line| line.contains(needle)),
        "expected {needle}: {}",
        failure.meta.logs.join("\n")
    );
}

#[test]
fn default_artifact_cannot_admit_policy() {
    let mut f = Fixture::new(true, false);
    f.reject_unchanged(f.open_ix(), "ExperimentalProfileDisabled");
    assert!(f.svm.get_account(&f.policy).is_none());
}

#[test]
fn identity_prevents_foreign_creator_and_changed_t0_squatting() {
    let mut f = Fixture::new(false, false);
    let mut stolen = f.open_ix();
    stolen.accounts[0].pubkey = f.outsider.pubkey();
    f.reject_unchanged(stolen, "IdentityMismatch");
    f.config.t0 += 1;
    f.reject_unchanged(f.open_ix(), "IdentityMismatch");
    f.config.t0 -= 1;
    f.run(f.open_ix()).unwrap();
    assert_eq!(f.p().config.t0, f.config.t0);
}

#[test]
fn consent_required_and_solo_owner_can_fill_all_roles() {
    let mut f = Fixture::new(false, false);
    let mut unsigned = f.open_ix();
    unsigned.accounts[1].is_signer = false;
    f.reject_unchanged(unsigned, "AccountNotSigner");
    let mut solo = Fixture::new(false, true);
    solo.fund();
    solo.run(solo.arm_ix()).unwrap();
    solo.run(solo.cancel_ix()).unwrap();
    solo.run(solo.refund_ix(FOUNDER, solo.source)).unwrap();
    solo.run(solo.refund_ix(TREASURY, solo.source)).unwrap();
    solo.conserved();
    assert_eq!(solo.balance(solo.source), SUPPLY);
}

#[test]
fn deposits_require_revoked_authorities_exact_amount_and_designated_role() {
    let mut f = Fixture::new(false, false);
    f.run(f.open_ix()).unwrap();
    f.reject_unchanged(f.deposit_ix(FOUNDER, 1), "WrongPrincipal");
    f.reject_unchanged(f.deposit_ix(2, 1), "InvalidConfig");
    let mut wrong = f.deposit_ix(FOUNDER, f.config.founder_amount);
    wrong.accounts[2].pubkey = f.outsider.pubkey();
    f.reject_unchanged(wrong, "Unauthorized");
    for freeze in [false, true] {
        let mut account = f.svm.get_account(&f.mint).unwrap();
        let mut mint = Mint::unpack(&account.data).unwrap();
        if freeze {
            mint.freeze_authority = COption::Some(f.creator.pubkey());
        } else {
            mint.mint_authority = COption::Some(f.creator.pubkey());
        }
        Mint::pack(mint, &mut account.data).unwrap();
        f.svm.set_account(f.mint, account).unwrap();
        f.reject_unchanged(
            f.deposit_ix(FOUNDER, f.config.founder_amount),
            "MintAuthorityLive",
        );
        let authority_type = if freeze {
            spl_token_interface::instruction::AuthorityType::FreezeAccount
        } else {
            spl_token_interface::instruction::AuthorityType::MintTokens
        };
        f.run(
            spl_token_interface::instruction::set_authority(
                &TOKEN_ID,
                &f.mint,
                None,
                authority_type,
                &f.creator.pubkey(),
                &[],
            )
            .unwrap(),
        )
        .unwrap();
    }
    f.run(f.deposit_ix(FOUNDER, f.config.founder_amount))
        .unwrap();
    f.conserved();
}

#[test]
fn both_exact_pools_required_duplicate_pool_rejected_and_deposit_time_does_not_retime() {
    let mut f = Fixture::new(false, false);
    f.run(f.open_ix()).unwrap();
    f.run(f.deposit_ix(FOUNDER, f.config.founder_amount))
        .unwrap();
    f.reject_unchanged(f.arm_ix(), "PoolsNotFunded");
    f.reject_unchanged(
        f.deposit_ix(FOUNDER, f.config.founder_amount),
        "already in use",
    );
    f.time(f.config.t0 - 1);
    f.run(f.deposit_ix(TREASURY, f.config.treasury_amount))
        .unwrap();
    f.run(f.arm_ix()).unwrap();
    f.reject_unchanged(f.activate_ix(), "T0Boundary");
    f.time(f.config.t0);
    f.run(f.activate_ix()).unwrap();
    assert_eq!(f.p().config.t0, START + 2 * PERIOD);
    assert_eq!(f.p().funded_mask, 3);
    f.conserved();
}

#[test]
fn t0_rejects_late_deposit_late_arm_and_cancel() {
    let mut f = Fixture::new(false, false);
    f.run(f.open_ix()).unwrap();
    f.time(f.config.t0);
    f.reject_unchanged(f.deposit_ix(FOUNDER, f.config.founder_amount), "T0Boundary");
    f.reject_unchanged(f.arm_ix(), "T0Boundary");
    f.reject_unchanged(f.cancel_ix(), "T0Boundary");
    let mut g = Fixture::new(false, false);
    g.fund();
    g.run(g.arm_ix()).unwrap();
    g.time(g.config.t0);
    g.reject_unchanged(g.cancel_ix(), "T0Boundary");
    g.reject_unchanged(g.expire_ix(), "InvalidState");
}

#[test]
fn unarmed_expiry_unlocks_only_original_depositors_refund_and_retains_tombstone() {
    let mut f = Fixture::new(false, false);
    f.run(f.open_ix()).unwrap();
    f.run(f.deposit_ix(FOUNDER, f.config.founder_amount))
        .unwrap();
    f.reject_unchanged(f.refund_ix(FOUNDER, f.source), "InvalidState");
    f.time(f.config.t0 - 1);
    f.reject_unchanged(f.expire_ix(), "T0Boundary");
    f.time(f.config.t0);
    f.run(f.expire_ix()).unwrap();
    f.reject_unchanged(f.refund_ix(FOUNDER, f.recipient), "ConstraintTokenOwner");
    let mut wrong = f.refund_ix(FOUNDER, f.source);
    wrong.accounts[0].pubkey = f.outsider.pubkey();
    f.reject_unchanged(wrong, "ConstraintHasOne");
    // A replacement account owned by the same depositor works if the source is closed.
    let replacement = Pubkey::new_unique();
    f.svm
        .set_account(replacement, token_account(f.mint, f.depositor.pubkey(), 0))
        .unwrap();
    f.run(f.refund_ix(FOUNDER, replacement)).unwrap();
    assert_eq!(f.balance(replacement), f.config.founder_amount);
    f.reject_unchanged(f.refund_ix(FOUNDER, replacement), "ZeroAmount");
    f.reject_unchanged(f.arm_ix(), "InvalidState");
    f.reject_unchanged(f.activate_ix(), "InvalidState");
    assert_eq!(f.p().state, CANCELLED);
    assert_eq!(f.v(FOUNDER).principal, f.config.founder_amount);
}

#[test]
fn armed_cancellation_needs_all_roles_and_refunds_principal_and_late_dust() {
    let mut f = Fixture::new(false, false);
    f.fund();
    f.run(f.arm_ix()).unwrap();
    let mut wrong = f.cancel_ix();
    wrong.accounts[1].pubkey = f.outsider.pubkey();
    f.reject_unchanged(wrong, "ConstraintHasOne");
    f.time(f.config.t0 - 1);
    f.run(f.cancel_ix()).unwrap();
    f.time(f.config.t0 + 1);
    f.run(f.refund_ix(FOUNDER, f.source)).unwrap();
    f.run(f.refund_ix(TREASURY, f.source)).unwrap();
    f.run(
        spl_token_interface::instruction::transfer_checked(
            &TOKEN_ID,
            &f.source,
            &f.mint,
            &f.vault_token(FOUNDER),
            &f.depositor.pubkey(),
            &[],
            7,
            9,
        )
        .unwrap(),
    )
    .unwrap();
    f.run(f.refund_ix(FOUNDER, f.source)).unwrap();
    f.conserved();
    assert_eq!(f.balance(f.source), SUPPLY);
}

#[test]
fn exact_t0_blocks_both_pools_and_treasury_can_use_mature_prelaunch_notice_at_t0_plus_one() {
    let mut f = Fixture::new(false, false);
    f.fund();
    f.run(f.approve_ix(0, UNIT, f.recipient)).unwrap();
    f.run(f.arm_ix()).unwrap();
    f.time(f.config.t0);
    f.run(f.activate_ix()).unwrap();
    f.report(UNIT);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "T0Boundary");
    f.reject_unchanged(f.release_ix(TREASURY, 1, 0), "T0Boundary");
    f.time(f.config.t0 + 1);
    f.run(f.release_ix(TREASURY, UNIT, 0)).unwrap();
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "CliffActive");
    f.conserved();
}

#[test]
fn founder_180_day_minus_one_exact_and_plus_one_are_enforced_on_real_tokens() {
    let mut f = Fixture::new(false, false);
    f.active();
    f.time(f.config.t0 + CLIFF - 1);
    f.report(f.config.shared_hard_cap);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "CliffActive");
    f.time(f.config.t0 + CLIFF);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "InvalidReport");
    f.report(f.config.shared_hard_cap);
    f.run(f.release_ix(FOUNDER, UNIT, 0)).unwrap();
    f.time(f.config.t0 + CLIFF + 1);
    f.run(f.release_ix(FOUNDER, f.config.founder_period_cap - UNIT, 0))
        .unwrap();
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "ReservedQuotaExceeded");
    assert_eq!(f.balance(f.founder_out), f.config.founder_period_cap);
    assert_eq!(f.v(FOUNDER).period, 6);
    f.conserved();
}

#[test]
fn skipped_periods_do_not_accumulate_and_cliff_is_not_full_unlock() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    f.reject_unchanged(
        f.release_ix(FOUNDER, f.config.founder_amount, 0),
        "ReservedQuotaExceeded",
    );
    f.run(f.release_ix(FOUNDER, f.config.founder_period_cap, 0))
        .unwrap();
    f.time(f.config.t0 + 20 * PERIOD);
    f.report(f.config.shared_hard_cap);
    f.reject_unchanged(
        f.release_ix(FOUNDER, 2 * f.config.founder_period_cap, 0),
        "ReservedQuotaExceeded",
    );
    f.run(f.release_ix(FOUNDER, f.config.founder_period_cap, 0))
        .unwrap();
    assert_eq!(f.v(FOUNDER).released_total, 2 * f.config.founder_period_cap);
    f.conserved();
}

#[test]
fn report_signer_sequence_freshness_and_clock_rollback_fail_closed() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    let now = f.config.t0 + CLIFF;
    let mut wrong = f.report_ix(UNIT, now, 2);
    wrong.accounts[0].pubkey = f.outsider.pubkey();
    f.reject_unchanged(wrong, "ConstraintHasOne");
    f.reject_unchanged(f.report_ix(UNIT, now, 1), "InvalidReport");
    f.reject_unchanged(f.report_ix(UNIT, now + 1, 2), "InvalidReport");
    f.time(now + f.config.max_report_age + 1);
    f.reject_unchanged(f.release_ix(FOUNDER, UNIT, 0), "InvalidReport");
    f.report(UNIT);
    f.time(now);
    f.reject_unchanged(f.release_ix(FOUNDER, UNIT, 0), "ClockRollback");
    f.reject_unchanged(f.report_ix(UNIT, now, 3), "ClockRollback");
}

#[test]
fn capacity_decrease_and_increase_preserve_used_counters() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    f.run(f.release_ix(FOUNDER, UNIT, 0)).unwrap();
    f.report(UNIT - 1);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "CapacityCorrectionPause");
    assert_eq!(f.p().shared_used, UNIT);
    f.report(5 * UNIT);
    f.run(f.release_ix(FOUNDER, UNIT, 0)).unwrap();
    f.report(u64::MAX);
    f.reject_unchanged(f.release_ix(FOUNDER, u64::MAX, 0), "Overflow");
    f.conserved();
}

#[test]
fn treasury_notice_boundary_recipient_owner_and_consumption_are_enforced() {
    let mut f = Fixture::new(false, false);
    f.active();
    f.time(f.config.t0 + PERIOD - 1);
    f.run(f.approve_ix(1, 2 * UNIT, f.recipient)).unwrap();
    let mature = f.config.t0 + 2 * PERIOD - 1;
    f.time(mature - 1);
    f.report(3 * UNIT);
    f.reject_unchanged(f.release_ix(TREASURY, UNIT, 1), "InvalidApproval");
    f.time(mature);
    f.run(f.release_ix(TREASURY, UNIT, 1)).unwrap();
    // SPL account address stays fixed but owner changes: original approval must fail.
    f.run(
        spl_token_interface::instruction::set_authority(
            &TOKEN_ID,
            &f.recipient,
            Some(&f.treasury.pubkey()),
            spl_token_interface::instruction::AuthorityType::AccountOwner,
            &f.outsider.pubkey(),
            &[],
        )
        .unwrap(),
    )
    .unwrap();
    f.reject_unchanged(f.release_ix(TREASURY, UNIT, 1), "InvalidApproval");
    f.time(mature + 1); // first second of period 2; approval 1 cannot carry over.
    f.report(3 * UNIT);
    f.reject_unchanged(f.release_ix(TREASURY, UNIT, 1), "InvalidApproval");
    let a: TreasuryApprovalV4 = f.read(f.approval(1));
    assert_eq!(a.consumed, UNIT);
    f.conserved();
}

#[test]
fn treasury_cannot_approve_self_current_period_or_replace_a_budget() {
    let mut f = Fixture::new(false, false);
    f.active();
    f.reject_unchanged(f.approve_ix(0, UNIT, f.recipient), "InvalidApproval");
    let self_account = Pubkey::new_unique();
    f.svm
        .set_account(self_account, token_account(f.mint, f.treasury.pubkey(), 0))
        .unwrap();
    f.reject_unchanged(f.approve_ix(1, UNIT, self_account), "InvalidApproval");
    f.run(f.approve_ix(1, UNIT, f.recipient)).unwrap();
    f.reject_unchanged(f.approve_ix(1, 2 * UNIT, f.recipient), "already in use");
}

#[test]
fn six_periods_reserve_each_pool_in_both_execution_orders() {
    for treasury_first in [true, false] {
        let mut f = Fixture::new(false, false);
        f.active();
        for period in 6..12 {
            f.run(f.approve_ix(period, f.config.treasury_period_cap, f.recipient))
                .unwrap();
        }
        for period in 6..12 {
            f.time(f.config.t0 + period as i64 * PERIOD);
            f.report(2_000_000 * UNIT);
            let f_quota = 800_000 * UNIT;
            let t_quota = 1_200_000 * UNIT;
            f.reject_unchanged(
                f.release_ix(TREASURY, f.config.treasury_period_cap, period),
                "ReservedQuotaExceeded",
            );
            if treasury_first {
                f.run(f.release_ix(TREASURY, t_quota, period)).unwrap();
                f.run(f.release_ix(FOUNDER, f_quota, 0)).unwrap();
            } else {
                f.run(f.release_ix(FOUNDER, f_quota, 0)).unwrap();
                f.run(f.release_ix(TREASURY, t_quota, period)).unwrap();
            }
            assert_eq!(f.p().shared_used, 2_000_000 * UNIT);
            assert_eq!(f.p().founder_period_used, f_quota);
            assert_eq!(f.p().treasury_period_used, t_quota);
            f.conserved();
        }
        assert_eq!(f.balance(f.founder_out), 4_800_000 * UNIT);
        assert_eq!(f.balance(f.recipient), 7_200_000 * UNIT);
        f.conserved();
    }
}

#[test]
fn failed_token_cpi_rolls_back_policy_vault_and_treasury_approval() {
    let mut f = Fixture::new(false, false);
    f.active();
    f.run(f.approve_ix(6, UNIT, f.recipient)).unwrap();
    f.time(f.config.t0 + CLIFF);
    f.report(3 * UNIT);
    // Fault injection models a token-program rejection, not an available freeze authority.
    let mut recipient = f.svm.get_account(&f.recipient).unwrap();
    let mut data = SplAccount::unpack(&recipient.data).unwrap();
    data.state = AccountState::Frozen;
    SplAccount::pack(data, &mut recipient.data).unwrap();
    f.svm.set_account(f.recipient, recipient).unwrap();
    f.reject_unchanged(f.release_ix(TREASURY, UNIT, 6), "frozen");
    assert_eq!(f.p().shared_used, 0);
    assert_eq!(f.v(TREASURY).released_total, 0);
    assert_eq!(f.read::<TreasuryApprovalV4>(f.approval(6)).consumed, 0);
    f.conserved();
}

#[test]
fn active_refund_wrong_founder_destination_and_role_swap_are_rejected() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    f.reject_unchanged(f.refund_ix(FOUNDER, f.source), "InvalidState");
    let mut wrong = f.release_ix(FOUNDER, UNIT, 0);
    wrong.accounts[5].pubkey = f.recipient;
    f.reject_unchanged(wrong, "Unauthorized");
    let mut swapped = f.release_ix(FOUNDER, UNIT, 0);
    swapped.accounts[0].pubkey = f.treasury.pubkey();
    f.reject_unchanged(swapped, "ConstraintHasOne");
    f.conserved();
}

#[test]
fn partial_withdrawals_preserve_weights_and_unused_shares_cannot_be_borrowed() {
    let mut f = Fixture::new(false, false);
    f.active();
    for period in [6, 7] {
        f.run(f.approve_ix(period, f.config.treasury_period_cap, f.recipient))
            .unwrap();
    }
    f.time(f.config.t0 + CLIFF);
    f.report(2_000_000 * UNIT);
    for amount in [200_000, 400_000, 200_000] {
        f.run(f.release_ix(FOUNDER, amount * UNIT, 0)).unwrap();
    }
    for amount in [300_000, 300_000, 600_000] {
        f.run(f.release_ix(TREASURY, amount * UNIT, 6)).unwrap();
    }
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "ReservedQuotaExceeded");
    f.time(f.config.t0 + 7 * PERIOD);
    f.report(2_000_000 * UNIT);
    f.run(f.release_ix(TREASURY, 1_200_000 * UNIT, 7)).unwrap();
    f.reject_unchanged(f.release_ix(TREASURY, 1, 7), "ReservedQuotaExceeded");
    assert_eq!(f.p().founder_period_used, 0);
    assert_eq!(f.p().founder_released_total, 800_000 * UNIT);
    f.conserved();
}

#[test]
fn a_drop_below_either_used_quota_pauses_both_pools_until_covered() {
    for first in [FOUNDER, TREASURY] {
        let mut f = Fixture::new(false, false);
        f.active();
        f.run(f.approve_ix(6, f.config.treasury_period_cap, f.recipient))
            .unwrap();
        f.time(f.config.t0 + CLIFF);
        f.report(2_000_000 * UNIT);
        let quotas = [800_000 * UNIT, 1_200_000 * UNIT];
        f.run(f.release_ix(first, quotas[usize::from(first)], 6))
            .unwrap();
        f.report(1_000_000 * UNIT);
        // The other pool has used nothing and some aggregate capacity may remain.
        // It still cannot act while either old allocation exceeds its new share.
        f.reject_unchanged(f.release_ix(1 - first, 1, 6), "CapacityCorrectionPause");
        f.reject_unchanged(f.release_ix(first, 1, 6), "CapacityCorrectionPause");
        f.report(2_000_000 * UNIT);
        f.run(f.release_ix(1 - first, quotas[usize::from(1 - first)], 6))
            .unwrap();
        assert_eq!(f.p().shared_used, 2_000_000 * UNIT);
        f.conserved();
    }
}

#[test]
fn annual_shared_balance_uses_prior_periods_without_double_debit() {
    for treasury_first in [true, false] {
        let mut f = Fixture::new(false, false);
        f.config.annual_rules[0].shared_cap = 3_000_000 * UNIT;
        f.rebind();
        f.active();
        for period in [6, 7, 8] {
            f.run(f.approve_ix(period, f.config.treasury_period_cap, f.recipient))
                .unwrap();
        }
        f.time(f.config.t0 + CLIFF);
        f.report(3_000_000 * UNIT);
        f.run(f.release_ix(TREASURY, 1_500_000 * UNIT, 6)).unwrap();
        f.run(f.release_ix(FOUNDER, 1_000_000 * UNIT, 0)).unwrap();
        f.time(f.config.t0 + 7 * PERIOD);
        f.report(3_000_000 * UNIT);
        // Only 500,000 remain for this budget epoch: F=200,000, T=300,000.
        let order = if treasury_first {
            [TREASURY, FOUNDER]
        } else {
            [FOUNDER, TREASURY]
        };
        for role in order {
            let half = if role == FOUNDER { 100_000 } else { 150_000 } * UNIT;
            f.run(f.release_ix(role, half, 7)).unwrap();
            f.run(f.release_ix(role, half, 7)).unwrap();
        }
        assert_eq!(f.p().founder_annual_used, 1_200_000 * UNIT);
        assert_eq!(f.p().treasury_annual_used, 1_800_000 * UNIT);
        f.time(f.config.t0 + 8 * PERIOD);
        f.report(u64::MAX);
        f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "ReservedQuotaExceeded");
        f.reject_unchanged(f.release_ix(TREASURY, 1, 8), "ReservedQuotaExceeded");
        f.conserved();
    }
}

#[test]
fn annual_boundary_changes_rate_without_resetting_lifetime_or_carrying_unused_budget() {
    let mut f = Fixture::new(false, false);
    f.config.annual_rules[1].release_bps = 250;
    f.config.annual_rules[1].shared_cap = 18_000_000 * UNIT;
    f.rebind();
    f.active();
    f.run(f.approve_ix(11, f.config.treasury_period_cap, f.recipient))
        .unwrap();
    f.run(f.approve_ix(12, f.config.treasury_period_cap, f.recipient))
        .unwrap();
    let boundary = f.config.t0 + 12 * PERIOD;
    f.time(boundary - 1);
    f.report(3_000_000 * UNIT);
    f.run(f.release_ix(FOUNDER, 1_000_000 * UNIT, 0)).unwrap();
    f.run(f.release_ix(TREASURY, 1_500_000 * UNIT, 11)).unwrap();
    f.time(boundary);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "InvalidReport");
    f.report(3_000_000 * UNIT);
    f.reject_unchanged(
        f.release_ix(FOUNDER, 500_000 * UNIT + 1, 0),
        "ReservedQuotaExceeded",
    );
    f.run(f.release_ix(FOUNDER, 500_000 * UNIT, 0)).unwrap();
    f.time(boundary + 1);
    f.run(f.release_ix(TREASURY, 1_000_000 * UNIT, 12)).unwrap();
    assert_eq!(f.p().annual_index, 1);
    assert_eq!(f.p().founder_annual_used, 500_000 * UNIT);
    assert_eq!(f.p().treasury_annual_used, 1_000_000 * UNIT);
    assert_eq!(f.p().founder_released_total, 1_500_000 * UNIT);
    assert_eq!(f.p().treasury_released_total, 2_500_000 * UNIT);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "ReservedQuotaExceeded");
    f.conserved();
}

#[test]
fn individual_annual_limit_binds_a_thirteenth_window_and_missing_future_input_blocks_release() {
    let mut f = Fixture::new(false, false);
    // Explicit 13-window test epoch exercises an annual bound that monthly
    // division by twelve alone cannot enforce. This is not a calendar choice.
    f.config.annual_rules[1].end_period = 25;
    f.rebind();
    f.active();
    f.run(f.approve_ix(24, f.config.treasury_period_cap, f.recipient))
        .unwrap();
    for period in 12..24 {
        f.time(f.config.t0 + period * PERIOD);
        f.report(3_000_000 * UNIT);
        f.run(f.release_ix(FOUNDER, 1_000_000 * UNIT, 0)).unwrap();
    }
    assert_eq!(f.p().founder_annual_used, 12_000_000 * UNIT);
    f.time(f.config.t0 + 24 * PERIOD);
    f.report(3_000_000 * UNIT);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "ReservedQuotaExceeded");
    f.run(f.release_ix(TREASURY, 1_500_000 * UNIT, 24)).unwrap();
    f.time(f.config.t0 + 25 * PERIOD);
    f.report(3_000_000 * UNIT);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "InvalidAnnualRule");
    assert_eq!(f.p().founder_released_total, 12_000_000 * UNIT);
    f.conserved();
}

#[test]
fn annual_inputs_are_immutable_bound_and_reject_invalid_ranges_rates_or_sources() {
    let mut f = Fixture::new(false, false);
    f.config.annual_rules[1].source_hash[0] ^= 1;
    f.reject_unchanged(f.open_ix(), "IdentityMismatch");
    f.rebind();
    f.config.annual_rules[0].release_bps = 501;
    f.reject_unchanged(f.open_ix(), "InvalidAnnualRule");
    f.config.annual_rules[0].release_bps = 500;
    f.config.annual_rules[1].start_period = 11;
    f.reject_unchanged(f.open_ix(), "InvalidAnnualRule");
    f.config.annual_rules[1].start_period = 13;
    f.reject_unchanged(f.open_ix(), "InvalidAnnualRule");
    f.config.annual_rules[1].start_period = 12;
    f.config.annual_rules[0].source_hash = [0; 32];
    f.reject_unchanged(f.open_ix(), "InvalidAnnualRule");
}

#[test]
fn zero_annual_rate_pauses_and_does_not_default_to_five_percent() {
    let mut f = Fixture::new(false, false);
    f.config.annual_rules[0].release_bps = 0;
    f.config.annual_rules[0].shared_cap = 0;
    f.rebind();
    f.at_cliff();
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "ReservedQuotaExceeded");
    assert_eq!(f.balance(f.founder_out), 0);
    f.conserved();
}

#[test]
fn export_raw_sbf_state_for_independent_verification() {
    let mut f = Fixture::new(false, false);
    f.active();
    f.run(f.approve_ix(6, f.config.treasury_period_cap, f.recipient))
        .unwrap();
    f.time(f.config.t0 + CLIFF);
    f.report(2_000_000 * UNIT);
    f.run(f.release_ix(TREASURY, 300_000 * UNIT, 6)).unwrap();
    f.run(f.release_ix(FOUNDER, 200_000 * UNIT, 0)).unwrap();
    f.conserved();
    if let Ok(path) = std::env::var("K4V_V4_SNAPSHOT_OUT") {
        let mut accounts = serde_json::Map::new();
        for (name, key) in [
            ("policy", f.policy),
            ("founder_vault", f.vault(FOUNDER)),
            ("treasury_vault", f.vault(TREASURY)),
            ("mint", f.mint),
            ("source", f.source),
            ("founder_token", f.vault_token(FOUNDER)),
            ("treasury_token", f.vault_token(TREASURY)),
            ("founder_destination", f.founder_out),
            ("treasury_destination", f.recipient),
            ("approval", f.approval(6)),
        ] {
            let account = f.svm.get_account(&key).unwrap();
            accounts.insert(
                name.to_owned(),
                serde_json::json!({"address":key.to_string(),
                "owner":account.owner.to_string(), "executable":account.executable,
                "data_hex":hex::encode(&account.data)}),
            );
        }
        let snapshot = serde_json::json!({"schema":"K4V-LAUNCH-V4-RAW-SNAPSHOT-v1",
            "scope":"AUTHOR_RUN_LOCAL_LITESVM", "program_id":ID.to_string(),
            "now":(f.config.t0 + CLIFF).to_string(), "accounts":accounts,
            "private_keys_serialized":false});
        std::fs::write(
            path,
            serde_json::to_string_pretty(&snapshot).unwrap() + "\n",
        )
        .unwrap();
    }
}

fn proposal(f: &Fixture, nonce: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[b"launch-v4-change", f.policy.as_ref(), &nonce.to_le_bytes()],
        &ID,
    )
    .0
}
fn change(f: &Fixture, kind: u8, recovery: bool, nonce: u64, successor: Pubkey) -> Instruction {
    let (first, second) = if recovery {
        (f.recovery[0].pubkey(), f.recovery[1].pubkey())
    } else {
        (f.p().controller, f.p().controller)
    };
    ix(
        accounts::ProposeChange {
            payer: f.outsider.pubkey(),
            initiator: first,
            cosigner: second,
            successor,
            policy: f.policy,
            proposal: proposal(f, nonce),
            system_program: solana_system_interface::program::ID,
        },
        instruction::ProposeChange {
            kind,
            recovery,
            nonce,
        },
    )
}
fn execute(f: &Fixture, nonce: u64) -> Instruction {
    ix(
        accounts::ExecuteChange {
            policy: f.policy,
            proposal: proposal(f, nonce),
        },
        instruction::ExecuteChange {},
    )
}
fn cancel_change(f: &Fixture, nonce: u64, first: Pubkey, second: Pubkey) -> Instruction {
    ix(
        accounts::CancelChange {
            initiator: first,
            cosigner: second,
            policy: f.policy,
            proposal: proposal(f, nonce),
        },
        instruction::CancelChange {},
    )
}
fn epoch_report(f: &Fixture, key: Pubkey, epoch: u64, sequence: u64, at: i64) -> Instruction {
    ix(
        accounts::Report {
            oracle: key,
            policy: f.policy,
        },
        instruction::ReportCapacity {
            capacity: 2_000_000 * UNIT,
            observed_at: at,
            sequence,
            epoch,
        },
    )
}
fn counters(p: &LaunchPolicyV4) -> Vec<u64> {
    vec![
        p.period,
        p.shared_used,
        p.founder_period_used,
        p.treasury_period_used,
        p.founder_released_total,
        p.treasury_released_total,
        p.annual_index as u64,
        p.founder_annual_used,
        p.treasury_annual_used,
    ]
}

#[test]
fn recovery_registration_requires_three_distinct_consented_identity_bound_keys() {
    let mut f = Fixture::new(false, false);
    f.config.recovery_keys[1] = f.config.recovery_keys[0];
    f.rebind();
    assert!(f.run(f.open_ix()).is_err());
    let mut f = Fixture::new(false, false);
    let mut missing = f.open_ix();
    missing
        .accounts
        .iter_mut()
        .find(|m| m.pubkey == f.recovery[2].pubkey())
        .unwrap()
        .is_signer = false;
    assert!(f.run(missing).is_err());
    f.config.recovery_keys.swap(0, 1);
    assert!(f.run(f.open_ix()).is_err()); // actors no longer match consented configuration
}

#[test]
fn key_changes_require_current_authority_and_successor_acceptance() {
    let mut f = Fixture::new(false, false);
    f.active();
    let mut wrong = change(&f, CHANGE_ORACLE, false, 1, f.outsider.pubkey());
    wrong.accounts[1].pubkey = f.founder.pubkey();
    wrong.accounts[2].pubkey = f.founder.pubkey();
    assert!(f.run(wrong).is_err());
    let mut unaccepted = change(&f, CHANGE_ORACLE, false, 1, f.outsider.pubkey());
    // Use another existing key, so it cannot acquire signer status through payer aliasing.
    unaccepted.accounts[3].pubkey = f.depositor.pubkey();
    unaccepted.accounts[3].is_signer = false;
    assert!(f.run(unaccepted).is_err());
    assert_eq!(f.p().change_sequence, 0);
    assert!(f
        .run(change(&f, CHANGE_ORACLE, false, 1, f.oracle.pubkey()))
        .is_err());
    assert!(f.run(change(&f, 2, false, 1, f.outsider.pubkey())).is_err());
}

#[test]
fn oracle_notice_minus_one_exact_and_plus_one_require_new_epoch_report() {
    for offset in [0, 1] {
        let mut f = Fixture::new(false, false);
        f.at_cliff();
        let start = f.config.t0 + CLIFF + 86_400;
        f.time(start);
        f.run(change(&f, CHANGE_ORACLE, false, 1, f.outsider.pubkey()))
            .unwrap();
        f.time(start + CHANGE_NOTICE - 1);
        let before = f.svm.get_account(&f.policy).unwrap().data;
        assert!(f.run(execute(&f, 1)).is_err());
        assert_eq!(f.svm.get_account(&f.policy).unwrap().data, before);
        f.time(start + CHANGE_NOTICE + offset);
        f.run_many(vec![execute(&f, 1)], f.outsider.pubkey())
            .unwrap();
        assert!(!f.last_signers.contains(&f.creator.pubkey()));
        assert_eq!(f.p().oracle_epoch, 1);
        assert!(!f.p().report_valid);
        assert!(f.run(f.release_ix(FOUNDER, 1, 9)).is_err());
        let now = start + CHANGE_NOTICE + offset;
        let seq = f.p().report_sequence + 1;
        assert!(f
            .run(epoch_report(&f, f.oracle.pubkey(), 1, seq, now))
            .is_err());
        assert!(f
            .run(epoch_report(&f, f.outsider.pubkey(), 0, seq, now))
            .is_err());
        assert!(f
            .run(epoch_report(&f, f.outsider.pubkey(), 1, seq, now - 1))
            .is_err());
        f.run(epoch_report(&f, f.outsider.pubkey(), 1, seq, now))
            .unwrap();
        f.run(f.release_ix(FOUNDER, 1, 9)).unwrap();
        assert!(f.run(execute(&f, 1)).is_err());
    }
}

#[test]
fn rotation_preserves_same_period_annual_principal_and_treasury_notice_accounting() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    let start = f.config.t0 + CLIFF + 86_400;
    f.time(start);
    f.run(f.approve_ix(9, 1_500_000 * UNIT, f.recipient))
        .unwrap();
    f.run(change(&f, CHANGE_ORACLE, false, 1, f.outsider.pubkey()))
        .unwrap();
    f.time(start + CHANGE_NOTICE - 1);
    f.report(2_000_000 * UNIT);
    f.run(f.release_ix(FOUNDER, 200_000 * UNIT, 9)).unwrap();
    f.run(f.release_ix(TREASURY, 300_000 * UNIT, 9)).unwrap();
    let before = counters(&f.p());
    let vaults = [
        f.svm.get_account(&f.vault(FOUNDER)).unwrap().data,
        f.svm.get_account(&f.vault(TREASURY)).unwrap().data,
    ];
    let approval_bytes = f.svm.get_account(&f.approval(9)).unwrap().data;
    let mut config = vec![];
    f.p().config.serialize(&mut config).unwrap();
    let sequence = f.p().report_sequence;
    let observed_at = f.p().report_at;
    f.time(start + CHANGE_NOTICE);
    f.run(execute(&f, 1)).unwrap();
    assert_eq!(counters(&f.p()), before);
    let mut after_config = vec![];
    f.p().config.serialize(&mut after_config).unwrap();
    assert_eq!(after_config, config);
    assert_eq!(f.p().report_sequence, sequence);
    assert_eq!(f.p().report_at, observed_at);
    assert_eq!(
        f.svm.get_account(&f.approval(9)).unwrap().data,
        approval_bytes
    );
    assert_eq!(
        f.svm.get_account(&f.vault(FOUNDER)).unwrap().data,
        vaults[0]
    );
    assert_eq!(
        f.svm.get_account(&f.vault(TREASURY)).unwrap().data,
        vaults[1]
    );
    f.run(epoch_report(
        &f,
        f.outsider.pubkey(),
        1,
        sequence + 1,
        start + CHANGE_NOTICE,
    ))
    .unwrap();
    assert!(f.run(f.release_ix(FOUNDER, 600_000 * UNIT + 1, 9)).is_err());
    f.run(f.release_ix(FOUNDER, 600_000 * UNIT, 9)).unwrap();
    f.run(f.release_ix(TREASURY, 900_000 * UNIT, 9)).unwrap();
    assert_eq!(f.p().shared_used, 2_000_000 * UNIT);
    f.conserved();
}

#[test]
fn returning_to_an_old_oracle_key_does_not_revive_old_epochs_or_sequences() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    let start = f.config.t0 + CLIFF;
    f.run(change(&f, CHANGE_ORACLE, false, 1, f.outsider.pubkey()))
        .unwrap();
    f.time(start + CHANGE_NOTICE);
    f.run(execute(&f, 1)).unwrap();
    f.run(epoch_report(
        &f,
        f.outsider.pubkey(),
        1,
        2,
        start + CHANGE_NOTICE,
    ))
    .unwrap();
    f.run(change(&f, CHANGE_ORACLE, false, 2, f.oracle.pubkey()))
        .unwrap();
    f.time(start + 2 * CHANGE_NOTICE);
    f.run(execute(&f, 2)).unwrap();
    assert_eq!(f.p().oracle_epoch, 2);
    assert!(f
        .run(epoch_report(
            &f,
            f.oracle.pubkey(),
            0,
            3,
            start + 2 * CHANGE_NOTICE
        ))
        .is_err());
    assert!(f
        .run(epoch_report(
            &f,
            f.oracle.pubkey(),
            2,
            2,
            start + 2 * CHANGE_NOTICE
        ))
        .is_err());
    f.run(epoch_report(
        &f,
        f.oracle.pubkey(),
        2,
        3,
        start + 2 * CHANGE_NOTICE,
    ))
    .unwrap();
}

#[test]
fn two_recovery_keys_restore_controller_without_lost_key_or_budget_reset() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    let identity = f.p().identity;
    let creator = f.creator.pubkey();
    let before = counters(&f.p());
    let tx = change(&f, CHANGE_CONTROLLER, true, 1, f.outsider.pubkey());
    f.run_many(vec![tx], f.outsider.pubkey()).unwrap();
    assert!(!f.last_signers.contains(&creator));
    assert!(f.run(cancel_change(&f, 1, creator, creator)).is_err());
    f.time(f.config.t0 + CLIFF + CHANGE_NOTICE);
    f.run_many(vec![execute(&f, 1)], f.outsider.pubkey())
        .unwrap();
    assert!(!f.last_signers.contains(&creator));
    assert_eq!(f.p().creator, creator);
    assert_eq!(f.p().identity, identity);
    assert_eq!(f.p().controller, f.outsider.pubkey());
    assert_eq!(f.p().controller_epoch, 1);
    assert_eq!(counters(&f.p()), before);
    let mut old = change(&f, CHANGE_ORACLE, false, 2, f.depositor.pubkey());
    old.accounts[1].pubkey = creator;
    old.accounts[2].pubkey = creator;
    assert!(f.run(old).is_err());
    f.run_many(
        vec![change(&f, CHANGE_ORACLE, false, 2, f.depositor.pubkey())],
        f.outsider.pubkey(),
    )
    .unwrap();
    assert!(!f.last_signers.contains(&creator));
}

#[test]
fn one_or_duplicate_or_unregistered_recovery_signature_cannot_recover() {
    let mut f = Fixture::new(false, false);
    f.active();
    for second in [f.recovery[0].pubkey(), f.founder.pubkey()] {
        let mut tx = change(&f, CHANGE_CONTROLLER, true, 1, f.outsider.pubkey());
        tx.accounts[2].pubkey = second;
        assert!(f.run(tx).is_err());
        assert_eq!(f.p().pending_change, 0);
    }
    let mut missing = change(&f, CHANGE_CONTROLLER, true, 1, f.outsider.pubkey());
    missing.accounts[2].is_signer = false;
    assert!(f.run(missing).is_err());
}

#[test]
fn recovery_quorum_atomically_evicts_hostile_pending_proposal_and_can_cancel_recovery() {
    let mut f = Fixture::new(false, false);
    f.active();
    f.run(change(&f, CHANGE_ORACLE, false, 1, f.depositor.pubkey()))
        .unwrap();
    assert!(f
        .run(change(&f, CHANGE_CONTROLLER, true, 2, f.outsider.pubkey()))
        .is_err());
    let cancel = cancel_change(&f, 1, f.recovery[0].pubkey(), f.recovery[1].pubkey());
    let next = change(&f, CHANGE_CONTROLLER, true, 2, f.outsider.pubkey());
    f.run_many(vec![cancel, next], f.outsider.pubkey()).unwrap();
    assert!(!f.last_signers.contains(&f.creator.pubkey()));
    assert_eq!(f.p().pending_change, 2);
    assert_eq!(
        f.read::<ChangeProposalV4>(proposal(&f, 1)).status,
        PROPOSAL_CANCELLED
    );
    assert!(f.run(execute(&f, 1)).is_err());
    let cancel = cancel_change(&f, 2, f.recovery[1].pubkey(), f.recovery[2].pubkey());
    f.run_many(vec![cancel], f.outsider.pubkey()).unwrap();
    assert_eq!(f.p().pending_change, 0);
    assert_eq!(f.p().change_sequence, 2);
    assert!(f
        .run(change(&f, CHANGE_CONTROLLER, true, 2, f.outsider.pubkey()))
        .is_err());
}

#[test]
fn accepted_successor_can_withdraw_and_cancelled_proposals_cannot_execute() {
    let mut f = Fixture::new(false, false);
    f.active();
    f.run(change(&f, CHANGE_ORACLE, true, 1, f.outsider.pubkey()))
        .unwrap();
    f.run_many(
        vec![cancel_change(
            &f,
            1,
            f.outsider.pubkey(),
            f.outsider.pubkey(),
        )],
        f.outsider.pubkey(),
    )
    .unwrap();
    f.time(f.config.t0 + CHANGE_NOTICE);
    assert!(f.run(execute(&f, 1)).is_err());
    assert_eq!(f.p().oracle, f.oracle.pubkey());
}

#[test]
fn recovery_clock_rollback_and_cross_policy_execution_are_rejected_atomically() {
    let mut f = Fixture::new(false, false);
    f.active();
    f.run(change(&f, CHANGE_ORACLE, false, 1, f.outsider.pubkey()))
        .unwrap();
    let before = f.svm.get_account(&f.policy).unwrap().data;
    f.time(f.config.t0 - 1);
    assert!(f.run(execute(&f, 1)).is_err());
    assert_eq!(f.svm.get_account(&f.policy).unwrap().data, before);
    f.time(f.config.t0 + CHANGE_NOTICE);
    let old_policy = f.policy;
    let old_proposal = proposal(&f, 1);
    f.config.t0 += 10 * PERIOD;
    f.rebind();
    f.run(f.open_ix()).unwrap();
    let attack = ix(
        accounts::ExecuteChange {
            policy: f.policy,
            proposal: old_proposal,
        },
        instruction::ExecuteChange {},
    );
    assert!(f.run(attack).is_err());
    assert_eq!(f.read::<LaunchPolicyV4>(old_policy).oracle_epoch, 0);
}

#[test]
fn cancellation_is_terminal_for_governance_and_preserves_original_depositor_exit() {
    let mut f = Fixture::new(false, false);
    f.fund();
    f.run(change(&f, CHANGE_CONTROLLER, true, 1, f.outsider.pubkey()))
        .unwrap();
    f.run(f.cancel_ix()).unwrap();
    assert!(f
        .run(change(&f, CHANGE_ORACLE, false, 2, f.depositor.pubkey()))
        .is_err());
    f.time(START + CHANGE_NOTICE);
    assert!(f.run(execute(&f, 1)).is_err());
    f.run(f.refund_ix(FOUNDER, f.source)).unwrap();
    f.run(f.refund_ix(TREASURY, f.source)).unwrap();
    assert_eq!(f.balance(f.source), SUPPLY);
}
#[test]
fn normal_controller_rotation_moves_lifecycle_authority_without_changing_identity() {
    let mut f = Fixture::new(false, false);
    f.config.t0 = START + 6 * PERIOD;
    f.rebind();
    f.fund();
    let id = f.p().identity;
    f.run(change(&f, CHANGE_CONTROLLER, false, 1, f.outsider.pubkey()))
        .unwrap();
    f.time(START + CHANGE_NOTICE);
    f.run(execute(&f, 1)).unwrap();
    assert_eq!(f.p().identity, id);
    assert_eq!(f.p().creator, f.creator.pubkey());
    assert!(f.run(f.arm_ix()).is_err());
    let mut arm = f.arm_ix();
    arm.accounts[0].pubkey = f.outsider.pubkey();
    f.run_many(vec![arm], f.outsider.pubkey()).unwrap();
    let mut cancel = f.cancel_ix();
    cancel.accounts[0].pubkey = f.outsider.pubkey();
    f.run_many(vec![cancel], f.outsider.pubkey()).unwrap();
    f.run(f.refund_ix(FOUNDER, f.source)).unwrap();
    f.run(f.refund_ix(TREASURY, f.source)).unwrap();
    assert_eq!(f.balance(f.source), SUPPLY);
}

#[test]
fn controller_recovery_does_not_claim_to_restore_lost_beneficiary_withdrawal_keys() {
    let mut f = Fixture::new(false, true);
    f.at_cliff();
    f.run_many(
        vec![change(&f, CHANGE_CONTROLLER, true, 1, f.outsider.pubkey())],
        f.outsider.pubkey(),
    )
    .unwrap();
    f.time(f.config.t0 + CLIFF + CHANGE_NOTICE);
    f.run_many(vec![execute(&f, 1)], f.outsider.pubkey())
        .unwrap();
    let before = f.balance(f.vault_token(FOUNDER));
    let mut withdrawal = f.release_ix(FOUNDER, 1, 9);
    withdrawal.accounts[0].pubkey = f.outsider.pubkey();
    assert!(f.run_many(vec![withdrawal], f.outsider.pubkey()).is_err());
    assert!(!f.last_signers.contains(&f.creator.pubkey()));
    assert_eq!(f.balance(f.vault_token(FOUNDER)), before);
    assert_eq!(f.p().founder, f.creator.pubkey());
}
