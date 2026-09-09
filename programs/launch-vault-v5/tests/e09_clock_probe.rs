#![cfg(feature = "test-profile")]
//! E-09 counterexample on unchanged v5 SBF. No repaired program is installed.
//! Prepared-policy probe: program/mint injection, fee airdrops and controlled
//! Clock are explicit fixtures. Transactions themselves carry real signatures.
use ::launch_vault_v5::{accounts, instruction, *};
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
            .join("../../target/v5-test/launch_vault_v5.so");
        svm.add_program_from_file(ID, artifact).unwrap();
        let keys: Vec<_> = (0..14).map(|_| Keypair::new()).collect();
        for key in &keys {
            svm.airdrop(&key.pubkey(), 1_000_000_000).unwrap();
        }
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../spec/LAUNCH_V5_IDENTITY_VECTOR_v1.json"
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
        let policy = Pubkey::find_program_address(&[b"launch-v5-policy", &identity], &ID).0;
        let record = Pubkey::find_program_address(
            &[
                b"launch-v5-key",
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
                b"launch-v5-withdraw",
                self.policy.as_ref(),
                &[role],
                &1u64.to_le_bytes(),
            ],
            &ID,
        )
        .0
    }

    fn instruction(&self, role: u8, recovery: bool, created_at: i64) -> Instruction {
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
                created_at,
            },
        )
    }

    fn sign(&mut self, instruction: Instruction) -> Transaction {
        self.svm.expire_blockhash();
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
    if let Ok(directory) = std::env::var("K4V_E09_PROBE_OUT_DIR") {
        let path = PathBuf::from(directory);
        std::fs::create_dir_all(&path).unwrap();
        let value = serde_json::json!({"schema":"K4V-E09-V5-CLOCK-PROBE-v1", "probe":name,
            "scope":"SIGNED_LOCAL_LITESVM_ON_UNCHANGED_V5_SBF", "program_id":ID.to_string(),
            "program_and_mint_injected":true, "clock_controlled":true, "fee_airdrops":true,
            "financial_transfers_tested":false, "public_chain_transactions":0, "private_keys_serialized":false, "cases":cases});
        std::fs::write(
            path.join(format!("{name}.json")),
            serde_json::to_string_pretty(&value).unwrap() + "\n",
        )
        .unwrap();
    }
}

#[test]
fn e09_already_signed_v5_submission_fails_after_clock_advances() {
    let mut cases = vec![];
    for role in 0..2 {
        for recovery in [false, true] {
            for delay in [0, 1, 30, 300] {
                let mut p = Probe::new();
                let tx = p.sign(p.instruction(role, recovery, NOW));
                let signature = tx.signatures[0].to_string();
                let blockhash = p.svm.latest_blockhash();
                let before = p.state(role);
                p.time(NOW + delay);
                assert_eq!(blockhash, p.svm.latest_blockhash());
                let result = p.svm.send_transaction(tx);
                if delay == 0 {
                    result.unwrap();
                    let a = p.svm.get_account(&p.proposal(role)).unwrap();
                    let q = WithdrawalProposalV5::try_deserialize(&mut a.data.as_slice()).unwrap();
                    assert_eq!(q.created_at, NOW);
                    assert_eq!(q.execute_after, NOW + CHANGE_NOTICE);
                } else {
                    let error = result.unwrap_err();
                    assert!(
                        error.meta.logs.iter().any(|s| s.contains("ProposalClock")),
                        "{error:?}"
                    );
                    assert_eq!(p.state(role), before); // Fee payer intentionally excluded.
                }
                cases.push(serde_json::json!({"role":role,"recovery":recovery,"delay_seconds":delay,
                    "outcome":if delay==0 {"ACCEPTED"} else {"PROPOSAL_CLOCK_REJECTED_STATE_UNCHANGED"},
                    "same_valid_blockhash":true,"local_signature":signature}));
            }
        }
    }
    receipt("clock_delay", cases);
}

#[test]
fn e09_timestamp_cannot_be_refreshed_without_new_signatures() {
    let mut cases = vec![];
    for role in 0..2 {
        for recovery in [false, true] {
            let mut p = Probe::new();
            let mut tx = p.sign(p.instruction(role, recovery, NOW));
            let original_signature = tx.signatures[0].to_string();
            let before = p.state(role);
            p.time(NOW + 1);
            tx.message.instructions[0].data[26..34].copy_from_slice(&(NOW + 1).to_le_bytes());
            let error = p.svm.send_transaction(tx).unwrap_err();
            assert!(
                format!("{:?}", error.err).contains("SignatureFailure"),
                "{error:?}"
            );
            assert_eq!(p.state(role), before);
            let refreshed = p.sign(p.instruction(role, recovery, NOW + 1));
            let new_signature = refreshed.signatures[0].to_string();
            p.svm.send_transaction(refreshed).unwrap();
            cases.push(serde_json::json!({"role":role,"recovery":recovery,"tamper":"SIGNATURE_FAILURE_STATE_UNCHANGED",
                "fresh_signing":"ACCEPTED_AT_NEW_EXACT_CLOCK","original_signature":original_signature,"new_signature":new_signature}));
        }
    }
    receipt("signature_refresh", cases);
}

#[test]
fn e09_new_blockhash_does_not_allow_reusing_a_consumed_role_nonce() {
    let mut cases = vec![];
    for role in 0..2 {
        for recovery in [false, true] {
            let mut p = Probe::new();
            let tx = p.sign(p.instruction(role, recovery, NOW));
            p.svm.send_transaction(tx).unwrap();
            let before = p.state(role);
            let replay = p.sign(p.instruction(role, recovery, NOW));
            let error = p.svm.send_transaction(replay).unwrap_err();
            assert!(
                error.meta.logs.iter().any(|s| s.contains("already in use")),
                "{error:?}"
            );
            assert_eq!(p.state(role), before);
            cases.push(
                serde_json::json!({"role":role,"recovery":recovery,"first":"ACCEPTED",
                "newly_signed_same_nonce":"PDA_ALREADY_IN_USE_STATE_UNCHANGED"}),
            );
        }
    }
    receipt("nonce_replay", cases);
}
