#![cfg(feature = "test-profile")]
//! E-10 signed admission-window probes on the isolated v6 candidate.
//! Prepared-policy probe: program/mint injection, fee airdrops and controlled
//! Clock are explicit fixtures. Transactions themselves carry real signatures.
use ::launch_vault_v6::{accounts, instruction, *};
use anchor_lang::{AccountDeserialize, AnchorDeserialize, InstructionData, ToAccountMetas};
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
use spl_token_interface::{state::Mint, ID as TOKEN};
use std::path::PathBuf;

const NOW: i64 = 1_700_000_000;

fn ix(a: impl ToAccountMetas, d: impl InstructionData) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: a.to_account_metas(None),
        data: d.data(),
    }
}

struct Probe {
    svm: LiteSVM,
    keys: Vec<Keypair>,
    policy: Pubkey,
    record: Pubkey,
    mint: Pubkey,
}

impl Probe {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/v6-test/launch_vault_v6.so");
        svm.add_program_from_file(ID, artifact).unwrap();
        let keys: Vec<_> = (0..14).map(|_| Keypair::new()).collect();
        for key in &keys {
            svm.airdrop(&key.pubkey(), 1_000_000_000).unwrap();
        }
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../spec/LAUNCH_V6_IDENTITY_VECTOR_v1.json"
        ))
        .unwrap();
        let encoded = hex::decode(vector["config_borsh_hex"].as_str().unwrap()).unwrap();
        let mut config = LaunchConfig::deserialize(&mut encoded.as_slice()).unwrap();
        config.t0 = NOW + 2 * PERIOD;
        config.recovery_keys = [4, 5, 6].map(|i| keys[i].pubkey());
        config.founder_recovery_keys = [7, 8, 9].map(|i| keys[i].pubkey());
        config.treasury_recovery_keys = [10, 11, 12].map(|i| keys[i].pubkey());
        let mint = Pubkey::new_unique();
        let mut data = vec![0; Mint::LEN];
        Mint::pack(
            Mint {
                mint_authority: COption::None,
                supply: 1_000_000_000_000_000_000,
                decimals: 9,
                is_initialized: true,
                freeze_authority: COption::None,
            },
            &mut data,
        )
        .unwrap();
        svm.set_account(
            mint,
            Account {
                lamports: 10_000_000,
                data,
                owner: TOKEN,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        let identity = identity(
            &keys[0].pubkey(),
            &mint,
            &keys[1].pubkey(),
            &keys[2].pubkey(),
            &keys[3].pubkey(),
            &[42; 32],
            &config,
        );
        let policy = Pubkey::find_program_address(&[b"launch-v6-policy", &identity], &ID).0;
        let record = Pubkey::find_program_address(
            &[
                b"launch-v6-key",
                policy.as_ref(),
                keys[13].pubkey().as_ref(),
            ],
            &ID,
        )
        .0;
        let opening = ix(
            accounts::OpenPolicy {
                creator: keys[0].pubkey(),
                founder: keys[1].pubkey(),
                treasury: keys[2].pubkey(),
                oracle: keys[3].pubkey(),
                recovery_one: keys[4].pubkey(),
                recovery_two: keys[5].pubkey(),
                recovery_three: keys[6].pubkey(),
                mint,
                policy,
                system_program: solana_system_interface::program::ID,
            },
            instruction::OpenPolicy {
                config,
                spec_hash: [42; 32],
                identity,
            },
        );
        let mut p = Self {
            svm,
            keys,
            policy,
            record,
            mint,
        };
        p.time(NOW);
        let tx = p.sign(opening);
        p.svm.send_transaction(tx).unwrap();
        let prepare = ix(
            accounts::PrepareWithdrawalKey {
                payer: p.keys[0].pubkey(),
                policy,
                record,
                system_program: solana_system_interface::program::ID,
            },
            instruction::PrepareWithdrawalKey {
                subject: p.keys[13].pubkey(),
            },
        );
        let tx = p.sign(prepare);
        p.svm.send_transaction(tx).unwrap();
        p
    }

    fn time(&mut self, now: i64) {
        let mut c = self.svm.get_sysvar::<Clock>();
        c.unix_timestamp = now;
        self.svm.set_sysvar(&c);
    }

    fn proposal(&self, role: u8) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"launch-v6-withdraw",
                self.policy.as_ref(),
                &[role],
                &1u64.to_le_bytes(),
            ],
            &ID,
        )
        .0
    }

    fn instruction(
        &self,
        role: u8,
        recovery: bool,
        valid_from: i64,
        valid_until: i64,
    ) -> Instruction {
        let current = 1 + role as usize;
        let first = 7 + role as usize * 3;
        let pair = if recovery {
            [first, first + 1]
        } else {
            [current, current]
        };
        ix(
            accounts::ProposeWithdrawal {
                payer: self.keys[0].pubkey(),
                initiator: self.keys[pair[0]].pubkey(),
                cosigner: self.keys[pair[1]].pubkey(),
                successor: self.keys[13].pubkey(),
                policy: self.policy,
                proposal: self.proposal(role),
                successor_record: self.record,
                system_program: solana_system_interface::program::ID,
            },
            instruction::ProposeWithdrawal {
                role,
                recovery,
                nonce: 1,
                epoch: 0,
                valid_from,
                valid_until,
                predecessor: self.keys[current].pubkey(),
            },
        )
    }

    fn sign(&mut self, instruction: Instruction) -> Transaction {
        self.svm.expire_blockhash();
        self.sign_current(instruction)
    }

    fn sign_current(&self, instruction: Instruction) -> Transaction {
        let signers: Vec<_> = self
            .keys
            .iter()
            .filter(|key| {
                key.pubkey() == self.keys[0].pubkey()
                    || instruction
                        .accounts
                        .iter()
                        .any(|a| a.pubkey == key.pubkey() && a.is_signer)
            })
            .collect();
        Transaction::new_signed_with_payer(
            &[instruction],
            Some(&self.keys[0].pubkey()),
            &signers,
            self.svm.latest_blockhash(),
        )
    }

    fn state(&self, role: u8) -> Vec<Option<Account>> {
        [self.policy, self.record, self.mint, self.proposal(role)]
            .map(|k| self.svm.get_account(&k))
            .to_vec()
    }
}

fn receipt(name: &str, cases: Vec<serde_json::Value>) {
    if let Ok(directory) = std::env::var("K4V_E10_PROBE_OUT_DIR") {
        let path = PathBuf::from(directory);
        std::fs::create_dir_all(&path).unwrap();
        let value = serde_json::json!({"schema":"K4V-E10-V6-SUBMISSION-PROBE-v1", "probe":name,
            "scope":"SIGNED_LOCAL_LITESVM_ON_V6_TEST_SBF", "program_id":ID.to_string(),
            "program_and_mint_injected":true, "clock_controlled":true, "fee_airdrops":true,
            "financial_transfers_tested":false, "public_chain_transactions":0, "private_keys_serialized":false, "cases":cases});
        std::fs::write(
            path.join(format!("{name}.json")),
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
}

fn assert_refused(p: &mut Probe, role: u8, transaction: Transaction, expected: &str) {
    let before = p.state(role);
    let error = p.svm.send_transaction(transaction).unwrap_err();
    assert!(format!("{error:?}").contains(expected), "{error:?}");
    assert_eq!(p.state(role), before); // Transaction fees are intentionally excluded.
}

fn proposal(p: &Probe, role: u8) -> WithdrawalProposalV6 {
    let a = p.svm.get_account(&p.proposal(role)).unwrap();
    assert_eq!(a.data.len(), 172);
    WithdrawalProposalV6::try_deserialize(&mut a.data.as_slice()).unwrap()
}

#[test]
fn signed_delayed_arrival_accepts_both_bounds_and_rejects_deadline_plus_one() {
    let mut cases = vec![];
    for role in 0..2 {
        for recovery in [false, true] {
            for delay in [0, 1, 30, 300, 301] {
                let mut p = Probe::new();
                let tx = p.sign(p.instruction(role, recovery, NOW, NOW + 300));
                let signature = tx.signatures[0].to_string();
                let hash = tx.message.recent_blockhash;
                p.time(NOW + delay);
                assert_eq!(hash, p.svm.latest_blockhash());
                if delay <= 300 {
                    p.svm.send_transaction(tx).unwrap();
                    let q = proposal(&p, role);
                    assert_eq!(
                        (q.valid_from, q.valid_until, q.created_at),
                        (NOW, NOW + 300, NOW + delay)
                    );
                    assert_eq!(q.execute_after, NOW + delay + CHANGE_NOTICE);
                    assert_eq!(q.expires_at, q.execute_after + WITHDRAWAL_EXECUTION_WINDOW);
                } else {
                    assert_refused(&mut p, role, tx, "SubmissionWindow");
                }
                cases.push(
                    serde_json::json!({"role":role,"recovery":recovery,"delay_seconds":delay,
                    "accepted":delay<=300,"local_signature":signature,"same_valid_blockhash":true}),
                );
            }
        }
    }
    receipt("signed_delay", cases);
}

#[test]
fn signed_intervals_reject_early_negative_reverse_oversize_and_overflow() {
    let safe = i64::MAX - CHANGE_NOTICE - WITHDRAWAL_EXECUTION_WINDOW;
    let mut cases = vec![];
    for role in 0..2 {
        for recovery in [false, true] {
            for (name, from, until, at, error) in [
                ("zero_width", NOW, NOW, NOW, ""),
                ("negative_start", -1, NOW, NOW, "SubmissionWindow"),
                ("reverse", NOW, NOW - 1, NOW, "SubmissionWindow"),
                ("oversize", NOW, NOW + 301, NOW, "SubmissionWindow"),
                ("early", NOW + 1, NOW + 301, NOW, "SubmissionWindow"),
                ("future_scheduled", NOW + 200, NOW + 300, NOW + 200, ""),
                ("latest_safe", safe - 300, safe, safe, ""),
                ("latest_overflow", safe - 299, safe + 1, safe, "Overflow"),
            ] {
                let mut p = Probe::new();
                let tx = p.sign(p.instruction(role, recovery, from, until));
                p.time(at);
                if error.is_empty() {
                    p.svm.send_transaction(tx).unwrap();
                    let q = proposal(&p, role);
                    assert_eq!(q.created_at, at);
                    assert_eq!(
                        q.expires_at,
                        at + CHANGE_NOTICE + WITHDRAWAL_EXECUTION_WINDOW
                    );
                } else {
                    assert_refused(&mut p, role, tx, error);
                }
                cases.push(
                    serde_json::json!({"role":role,"recovery":recovery,"case":name,
                    "accepted":error.is_empty(),"expected_error":error}),
                );
            }
        }
    }
    receipt("interval_boundaries", cases);
}

#[test]
fn all_intent_bytes_and_all_required_signatures_are_bound() {
    let mut cases = vec![];
    for role in 0..2 {
        for recovery in [false, true] {
            let mut p = Probe::new();
            // Role, mode, nonce, epoch, both interval endpoints, predecessor.
            for (field, offset) in [
                ("role", 8),
                ("mode", 9),
                ("nonce", 10),
                ("epoch", 18),
                ("valid_from", 26),
                ("valid_until", 34),
                ("predecessor", 42),
            ] {
                let mut tx = p.sign(p.instruction(role, recovery, NOW, NOW + 300));
                tx.message.instructions[0].data[offset] ^= 1;
                assert_refused(&mut p, role, tx, "SignatureFailure");
                cases.push(serde_json::json!({"role":role,"recovery":recovery,"changed":field,"accepted":false}));
            }
            for field in ["program", "policy", "successor"] {
                let mut tx = p.sign(p.instruction(role, recovery, NOW, NOW + 300));
                let index = match field {
                    "program" => tx.message.instructions[0].program_id_index,
                    "policy" => tx.message.instructions[0].accounts[4],
                    _ => tx.message.instructions[0].accounts[3],
                } as usize;
                tx.message.account_keys[index] = Pubkey::new_unique();
                assert_refused(&mut p, role, tx, "SignatureFailure");
                cases.push(serde_json::json!({"role":role,"recovery":recovery,"changed":field,"accepted":false}));
            }
            let tx = p.sign(p.instruction(role, recovery, NOW, NOW + 300));
            for i in 0..tx.signatures.len() {
                let mut missing = tx.clone();
                missing.signatures[i] = Default::default();
                assert_refused(&mut p, role, missing, "SignatureFailure");
                cases.push(serde_json::json!({"role":role,"recovery":recovery,"missing_signature":i,"accepted":false}));
            }
            let mut changed = tx;
            changed.message.instructions[0].data[34..42]
                .copy_from_slice(&(NOW + 299).to_le_bytes());
            changed.partial_sign(&[&p.keys[0]], changed.message.recent_blockhash);
            assert_refused(&mut p, role, changed, "SignatureFailure");
            cases.push(serde_json::json!({"role":role,"recovery":recovery,"changed":"fee_payer_only_resigned","accepted":false}));
            // A fully re-signed wrong predecessor still fails the on-chain check.
            let mut wrong = p.instruction(role, recovery, NOW, NOW + 300);
            wrong.data[42..74].copy_from_slice(p.keys[0].pubkey().as_ref());
            let tx = p.sign(wrong);
            assert_refused(&mut p, role, tx, "InvalidChange");
            cases.push(serde_json::json!({"role":role,"recovery":recovery,"changed":"resigned_wrong_predecessor","accepted":false}));
            let tx = p.sign(p.instruction(role, recovery, NOW, NOW + 299));
            p.time(NOW + 30);
            p.svm.send_transaction(tx).unwrap();
            cases.push(serde_json::json!({"role":role,"recovery":recovery,"changed":"all_signers_refreshed","accepted":true}));
        }
    }
    receipt("signature_binding", cases);
}

#[test]
fn stale_blockhash_is_rejected_independently_of_valid_application_window() {
    let mut cases = vec![];
    for role in 0..2 {
        for recovery in [false, true] {
            let mut p = Probe::new();
            let tx = p.sign(p.instruction(role, recovery, NOW, NOW + 300));
            // Deliberately expire a formerly valid hash after signing; the
            // application interval remains valid in this controlled local bank.
            p.svm.expire_blockhash();
            assert_refused(&mut p, role, tx, "BlockhashNotFound");
            let tx = p.sign(p.instruction(role, recovery, NOW, NOW + 300));
            p.time(NOW + 1);
            p.svm.send_transaction(tx).unwrap();
            cases.push(serde_json::json!({"role":role,"recovery":recovery,
                "expired_hash_rejected":true,"freshly_signed_valid_hash_accepted":true}));
        }
    }
    receipt("blockhash_separation", cases);
}

#[test]
fn competing_signed_messages_cannot_reuse_nonce_after_admission_or_cancellation() {
    let mut cases = vec![];
    for role in 0..2 {
        for recovery in [false, true] {
            let mut p = Probe::new();
            let accepted = p.sign(p.instruction(role, recovery, NOW, NOW + 300));
            let competing = p.sign_current(p.instruction(role, recovery, NOW, NOW + 299));
            p.svm.send_transaction(accepted).unwrap();
            assert_refused(&mut p, role, competing, "already in use");
            let cancel = ix(
                accounts::CancelWithdrawal {
                    initiator: p.keys[13].pubkey(),
                    cosigner: p.keys[13].pubkey(),
                    policy: p.policy,
                    proposal: p.proposal(role),
                    successor_record: p.record,
                },
                instruction::CancelWithdrawal {},
            );
            let cancel = p.sign(cancel);
            p.svm.send_transaction(cancel).unwrap();
            assert_eq!(proposal(&p, role).status, PROPOSAL_CANCELLED);
            let replay = p.sign(p.instruction(role, recovery, NOW, NOW + 298));
            assert_refused(&mut p, role, replay, "already in use");
            cases.push(serde_json::json!({"role":role,"recovery":recovery,"competing_rejected":true,
                "successor_cancellation_accepted":true,"fresh_hash_replay_after_cancellation_rejected":true}));
        }
    }
    receipt("nonce_race", cases);
}
