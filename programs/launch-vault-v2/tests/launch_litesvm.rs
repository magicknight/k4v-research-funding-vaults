#![cfg(feature = "test-profile")]

use ::launch_vault_v2::{accounts, instruction, *};
use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
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
    mint: Pubkey,
    source: Pubkey,
    founder_out: Pubkey,
    recipient: Pubkey,
    policy: Pubkey,
    config: LaunchConfig,
    hash: [u8; 32],
}

impl Fixture {
    fn new(disabled: bool, solo: bool) -> Self {
        let mut svm = LiteSVM::new();
        let directory = if disabled { "v2-disabled" } else { "v2-test" };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/{directory}/launch_vault_v2.so"));
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
        for key in [
            &creator, &founder, &treasury, &depositor, &oracle, &outsider,
        ] {
            svm.expire_blockhash();
            svm.airdrop(&key.pubkey(), 10_000_000_000).unwrap();
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
            treasury_period_cap: 3_000_000 * UNIT,
            shared_hard_cap: 3_000_000 * UNIT,
            max_report_age: 86_400,
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
        let policy = Pubkey::find_program_address(&[b"launch-v2-policy", &hash], &ID).0;
        let mut f = Self {
            svm,
            creator,
            founder,
            treasury,
            depositor,
            oracle,
            outsider,
            mint,
            source,
            founder_out,
            recipient,
            policy,
            config,
            hash,
        };
        f.time(START);
        f
    }

    fn run(&mut self, instruction: Instruction) -> Outcome {
        self.svm.expire_blockhash();
        let mut signers: Vec<&Keypair> = vec![&self.creator];
        for k in [
            &self.founder,
            &self.treasury,
            &self.depositor,
            &self.oracle,
            &self.outsider,
        ] {
            if instruction
                .accounts
                .iter()
                .any(|m| m.pubkey == k.pubkey() && m.is_signer)
                && !signers.iter().any(|s| s.pubkey() == k.pubkey())
            {
                signers.push(k);
            }
        }
        self.svm
            .send_transaction(Transaction::new_signed_with_payer(
                &[instruction],
                Some(&self.creator.pubkey()),
                &signers,
                self.svm.latest_blockhash(),
            ))
            .map_err(Box::new)
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
        Pubkey::find_program_address(&[b"launch-v2-vault", self.policy.as_ref(), &[role]], &ID).0
    }

    fn vault_token(&self, role: u8) -> Pubkey {
        Pubkey::find_program_address(&[b"launch-v2-token", self.vault(role).as_ref()], &ID).0
    }

    fn approval(&self, period: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"launch-v2-approval",
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

    fn p(&self) -> LaunchPolicyV2 {
        self.read(self.policy)
    }
    fn v(&self, role: u8) -> LaunchVaultV2 {
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
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "CapacityExceeded");
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
        "CapacityExceeded",
    );
    f.run(f.release_ix(FOUNDER, f.config.founder_period_cap, 0))
        .unwrap();
    f.time(f.config.t0 + 60 * PERIOD);
    f.report(f.config.shared_hard_cap);
    f.reject_unchanged(
        f.release_ix(FOUNDER, 2 * f.config.founder_period_cap, 0),
        "CapacityExceeded",
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
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "CapacityExceeded");
    assert_eq!(f.p().shared_used, UNIT);
    f.report(2 * UNIT);
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
    let a: TreasuryApprovalV2 = f.read(f.approval(1));
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
fn shared_capacity_is_conserved_in_both_orders_but_starvation_remains_open() {
    for treasury_first in [true, false] {
        let mut f = Fixture::new(false, false);
        f.active();
        f.run(f.approve_ix(6, f.config.treasury_period_cap, f.recipient))
            .unwrap();
        f.time(f.config.t0 + CLIFF);
        f.report(f.config.shared_hard_cap);
        if treasury_first {
            f.run(f.release_ix(TREASURY, f.config.shared_hard_cap, 6))
                .unwrap();
            // Explicit witness: TEST_ONLY legacy allocation has no founder reservation.
            f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "CapacityExceeded");
        } else {
            f.run(f.release_ix(FOUNDER, f.config.founder_period_cap, 0))
                .unwrap();
            f.run(f.release_ix(
                TREASURY,
                f.config.shared_hard_cap - f.config.founder_period_cap,
                6,
            ))
            .unwrap();
        }
        assert_eq!(f.p().shared_used, f.config.shared_hard_cap);
        assert_eq!(
            f.balance(f.founder_out) + f.balance(f.recipient),
            f.config.shared_hard_cap
        );
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
    assert_eq!(f.read::<TreasuryApprovalV2>(f.approval(6)).consumed, 0);
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
