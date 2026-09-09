#![cfg(feature = "test-profile")]
use ::upgrade_gate_v1::{
    accounts, instruction, UpgradeGateV1, CANCELLED, EXECUTED, ID, NOTICE, PENDING,
};
use anchor_lang::solana_program::bpf_loader_upgradeable::ID as LOADER_ID;
use anchor_lang::solana_program::sysvar::SysvarId;
use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use litesvm::LiteSVM;
use solana_clock::Clock;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_loader_v3_interface::{
    get_program_data_address, instruction as loader, state::UpgradeableLoaderState as LoaderState,
};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::path::PathBuf;

const START: i64 = 1_700_000_000;
type Outcome =
    Result<litesvm::types::TransactionMetadata, Box<litesvm::types::FailedTransactionMetadata>>;
fn ix(a: impl ToAccountMetas, d: impl InstructionData) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: a.to_account_metas(None),
        data: d.data(),
    }
}
fn key(label: &str) -> Keypair {
    Keypair::new_from_array(solana_sha256_hasher::hash(label.as_bytes()).to_bytes())
}
fn artifact(path: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(path),
    )
    .unwrap()
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    solana_sha256_hasher::hash(bytes).to_bytes()
}

struct Fixture {
    svm: LiteSVM,
    owner: Keypair,
    members: [Keypair; 3],
    outsider: Keypair,
    gate_program: Keypair,
    target: Keypair,
    gate: Pubkey,
    sent: u64,
    accepted: u64,
    last_signers: Vec<Pubkey>,
    last_signature: String,
}

impl Fixture {
    fn new(disabled: bool, seal: bool) -> Self {
        let mut svm = LiteSVM::new();
        let owner = Keypair::new();
        let members = [Keypair::new(), Keypair::new(), Keypair::new()];
        let outsider = Keypair::new();
        for k in [&owner, &members[0], &members[1], &members[2], &outsider] {
            svm.expire_blockhash();
            svm.airdrop(&k.pubkey(), 100_000_000_000).unwrap();
        }
        let gate_program = key("k4v-upgrade-gate-v1-test-only");
        let target = key("k4v-launch-vault-v4-recovery-test-only");
        assert_eq!(gate_program.pubkey(), ID);
        let gate =
            Pubkey::find_program_address(&[b"upgrade-gate-v1", target.pubkey().as_ref()], &ID).0;
        let mut f = Self {
            svm,
            owner,
            members,
            outsider,
            gate_program,
            target,
            gate,
            sent: 0,
            accepted: 0,
            last_signers: vec![],
            last_signature: String::new(),
        };
        f.time(START);
        let path = if disabled {
            "gate-disabled/upgrade_gate_v1.so"
        } else {
            "gate-test/upgrade_gate_v1.so"
        };
        f.deploy(true, &artifact(path), seal);
        f.deploy(false, &artifact("v4-disabled/launch_vault_v4.so"), false);
        f
    }
    fn time(&mut self, timestamp: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = timestamp;
        self.svm.set_sysvar(&clock);
    }
    fn run(&mut self, ixs: Vec<Instruction>, extra: &[&Keypair], payer: Pubkey) -> Outcome {
        // Actual loader deployment and upgrade require distinct slots.
        let slot = self.svm.get_sysvar::<Clock>().slot;
        self.svm.warp_to_slot(slot + 1);
        self.svm.expire_blockhash();
        let mut all = vec![ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)];
        all.extend(ixs);
        let keys = [
            &self.owner,
            &self.members[0],
            &self.members[1],
            &self.members[2],
            &self.outsider,
            &self.gate_program,
            &self.target,
        ];
        let mut signers: Vec<&Keypair> = vec![];
        for k in keys.into_iter().chain(extra.iter().copied()) {
            if (k.pubkey() == payer
                || all.iter().any(|ix| {
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
        let tx = Transaction::new_signed_with_payer(
            &all,
            Some(&payer),
            &signers,
            self.svm.latest_blockhash(),
        );
        self.last_signature = tx.signatures[0].to_string();
        self.sent += 1;
        let result = self.svm.send_transaction(tx).map_err(Box::new);
        if result.is_ok() {
            self.accepted += 1;
        }
        result
    }
    fn call(&mut self, instruction: Instruction) -> Outcome {
        self.run(vec![instruction], &[], self.outsider.pubkey())
    }
    fn upload(&mut self, bytes: &[u8]) -> Keypair {
        let buffer = Keypair::new();
        self.run(
            loader::create_buffer(
                &self.owner.pubkey(),
                &buffer.pubkey(),
                &self.owner.pubkey(),
                self.svm
                    .minimum_balance_for_rent_exemption(LoaderState::size_of_buffer(bytes.len())),
                bytes.len(),
            )
            .unwrap(),
            &[&buffer],
            self.owner.pubkey(),
        )
        .unwrap();
        for (i, chunk) in bytes.chunks(900).enumerate() {
            self.call(loader::write(
                &buffer.pubkey(),
                &self.owner.pubkey(),
                (i * 900) as u32,
                chunk.to_vec(),
            ))
            .unwrap();
        }
        buffer
    }
    fn deploy(&mut self, is_gate: bool, bytes: &[u8], seal: bool) {
        let buffer = self.upload(bytes);
        let target = if is_gate {
            self.gate_program.pubkey()
        } else {
            self.target.pubkey()
        };
        self.run(
            loader::deploy_with_max_program_len(
                &self.owner.pubkey(),
                &target,
                &buffer.pubkey(),
                &self.owner.pubkey(),
                self.svm
                    .minimum_balance_for_rent_exemption(LoaderState::size_of_program()),
                bytes.len() + 64_000,
            )
            .unwrap(),
            &[],
            self.owner.pubkey(),
        )
        .unwrap();
        let deployed = self.programdata(target);
        let offset = LoaderState::size_of_programdata_metadata();
        assert_eq!(&deployed.data[offset..offset + bytes.len()], bytes);
        assert!(deployed.data[offset + bytes.len()..]
            .iter()
            .all(|b| *b == 0));
        if seal {
            self.call(loader::set_upgrade_authority(
                &target,
                &self.owner.pubkey(),
                None,
            ))
            .unwrap();
        }
    }
    fn init_ix(&self) -> Instruction {
        ix(
            accounts::InitializeGate {
                payer: self.outsider.pubkey(),
                member_one: self.members[0].pubkey(),
                member_two: self.members[1].pubkey(),
                member_three: self.members[2].pubkey(),
                current_authority: self.owner.pubkey(),
                target: self.target.pubkey(),
                programdata: get_program_data_address(&self.target.pubkey()),
                gate_programdata: get_program_data_address(&ID),
                gate: self.gate,
                loader: LOADER_ID,
                system_program: solana_system_interface::program::ID,
            },
            instruction::InitializeGate {},
        )
    }
    fn init(&mut self) {
        self.call(self.init_ix()).unwrap();
    }
    fn seal_buffer(&mut self, buffer: &Keypair) {
        self.call(loader::set_buffer_authority(
            &buffer.pubkey(),
            &self.owner.pubkey(),
            &self.gate,
        ))
        .unwrap();
    }
    fn proposal_ix(&self, buffer: Pubkey, nonce: u64, code_hash: [u8; 32]) -> Instruction {
        // Member zero is deliberately absent: one lost committee key is tolerated.
        ix(
            accounts::ProposeUpgrade {
                member_one: self.members[1].pubkey(),
                member_two: self.members[2].pubkey(),
                return_authority: self.owner.pubkey(),
                gate: self.gate,
                buffer,
            },
            instruction::ProposeUpgrade { nonce, code_hash },
        )
    }
    fn execute_ix(&self, buffer: Pubkey, nonce: u64) -> Instruction {
        ix(
            accounts::ExecuteUpgrade {
                gate: self.gate,
                target: self.target.pubkey(),
                programdata: get_program_data_address(&self.target.pubkey()),
                buffer,
                spill: self.owner.pubkey(),
                rent: anchor_lang::prelude::Rent::id(),
                clock: anchor_lang::prelude::Clock::id(),
                loader: LOADER_ID,
            },
            instruction::ExecuteUpgrade { nonce },
        )
    }
    fn return_accounts(&self, buffer: Pubkey, recipient: Pubkey) -> accounts::ReturnBuffer {
        accounts::ReturnBuffer {
            member_one: self.members[1].pubkey(),
            member_two: self.members[2].pubkey(),
            gate: self.gate,
            buffer,
            recipient,
            loader: LOADER_ID,
        }
    }
    fn g(&self) -> UpgradeGateV1 {
        UpgradeGateV1::try_deserialize(
            &mut self.svm.get_account(&self.gate).unwrap().data.as_slice(),
        )
        .unwrap()
    }
    fn programdata(&self, program: Pubkey) -> solana_account::Account {
        self.svm
            .get_account(&get_program_data_address(&program))
            .unwrap()
    }
    fn authority(&self, program: Pubkey) -> Option<Pubkey> {
        match bincode::deserialize::<LoaderState>(&self.programdata(program).data).unwrap() {
            LoaderState::ProgramData {
                upgrade_authority_address,
                ..
            } => upgrade_authority_address,
            _ => panic!("not ProgramData"),
        }
    }
}

#[test]
fn gate_default_admission_and_mutable_gate_are_rejected_before_authority_transfer() {
    for (disabled, seal) in [(true, true), (false, false)] {
        let mut f = Fixture::new(disabled, seal);
        assert!(f.call(f.init_ix()).is_err());
        assert_eq!(f.authority(f.target.pubkey()), Some(f.owner.pubkey()));
        assert!(f.svm.get_account(&f.gate).is_none());
    }
}

#[test]
fn gate_initialization_requires_exact_loader_authority_and_distinct_consent() {
    let mut f = Fixture::new(false, true);
    let mut wrong = f.init_ix();
    wrong.accounts[4].pubkey = f.outsider.pubkey();
    assert!(f.call(wrong).is_err());
    let mut duplicate = f.init_ix();
    duplicate.accounts[2].pubkey = f.members[0].pubkey();
    assert!(f.call(duplicate).is_err());
    let mut missing = f.init_ix();
    missing.accounts[3].is_signer = false;
    assert!(f.call(missing).is_err());
    f.init();
    assert_eq!(f.authority(ID), None);
    assert_eq!(f.authority(f.target.pubkey()), Some(f.gate));
    assert!(f
        .call(loader::set_upgrade_authority(
            &f.target.pubkey(),
            &f.owner.pubkey(),
            Some(&f.owner.pubkey())
        ))
        .is_err());
    assert!(f
        .call(loader::set_upgrade_authority(
            &ID,
            &f.owner.pubkey(),
            Some(&f.owner.pubkey())
        ))
        .is_err());
}

#[test]
fn queued_upgrade_enforces_quorum_hash_buffer_lock_notice_and_real_loader_cpi() {
    let mut f = Fixture::new(false, true);
    f.init();
    let bytes = artifact("v4-test/launch_vault_v4.so");
    let buffer = f.upload(&bytes);
    assert!(f
        .call(f.proposal_ix(buffer.pubkey(), 1, hash(&bytes)))
        .is_err()); // not yet locked
    f.seal_buffer(&buffer);
    let mut one = f.proposal_ix(buffer.pubkey(), 1, hash(&bytes));
    one.accounts[1].pubkey = f.members[1].pubkey();
    assert!(f.call(one).is_err());
    let mut foreign = f.proposal_ix(buffer.pubkey(), 1, hash(&bytes));
    foreign.accounts[1].pubkey = f.outsider.pubkey();
    assert!(f.call(foreign).is_err());
    assert!(f.call(f.proposal_ix(buffer.pubkey(), 1, [42; 32])).is_err());
    f.call(f.proposal_ix(buffer.pubkey(), 1, hash(&bytes)))
        .unwrap();
    assert!(!f.last_signers.contains(&f.members[0].pubkey()));
    let proposal_sig = f.last_signature.clone();
    assert_eq!(f.g().status, PENDING);
    assert!(f
        .call(loader::write(
            &buffer.pubkey(),
            &f.owner.pubkey(),
            0,
            vec![0]
        ))
        .is_err());
    assert!(f
        .call(loader::upgrade(
            &f.target.pubkey(),
            &buffer.pubkey(),
            &f.owner.pubkey(),
            &f.owner.pubkey()
        ))
        .is_err());
    let maturity = f.g().execute_after;
    f.time(maturity - 1);
    let before = f.svm.get_account(&f.gate).unwrap().data;
    assert!(f.call(f.execute_ix(buffer.pubkey(), 1)).is_err());
    assert_eq!(f.svm.get_account(&f.gate).unwrap().data, before);
    let before_program = f.programdata(f.target.pubkey()).data;
    f.time(maturity);
    let mut wrong_spill = f.execute_ix(buffer.pubkey(), 1);
    wrong_spill.accounts[4].pubkey = f.outsider.pubkey();
    assert!(f.call(wrong_spill).is_err());
    f.call(f.execute_ix(buffer.pubkey(), 1)).unwrap();
    assert_eq!(f.last_signers, vec![f.outsider.pubkey()]);
    let execute_sig = f.last_signature.clone();
    assert_eq!(f.g().status, EXECUTED);
    assert!(f.call(f.execute_ix(buffer.pubkey(), 1)).is_err());
    let after = f.programdata(f.target.pubkey());
    assert_ne!(after.data, before_program);
    let offset = LoaderState::size_of_programdata_metadata();
    assert_eq!(&after.data[offset..offset + bytes.len()], bytes.as_slice());
    assert!(after.data[offset + bytes.len()..].iter().all(|b| *b == 0));
    assert_eq!(f.authority(f.target.pubkey()), Some(f.gate));
    assert_eq!(f.authority(ID), None);
    if let Ok(path) = std::env::var("K4V_GATE_RECEIPT_OUT") {
        let header = |program: Pubkey| {
            let a = f.programdata(program);
            serde_json::json!({"program":program.to_string(), "programdata":get_program_data_address(&program).to_string(),
                "owner":a.owner.to_string(),"executable":a.executable,"data_len":a.data.len(),
                "loader_header_hex":hex::encode(&a.data[..offset]),"authority":f.authority(program).map(|x| x.to_string())})
        };
        let receipt = serde_json::json!({
            "schema":"K4V-UPGRADE-GATE-LOCAL-REAL-LOADER-v1",
            "environment":"LiteSVM native upgradeable loader, controlled Clock/slots; no public RPC",
            "program_injection_used":false,"public_chain_transactions":0,
            "gate":header(ID),"target":header(f.target.pubkey()),"gate_account":f.gate.to_string(),
            "gate_code_hash":hex::encode(hash(&artifact("gate-test/upgrade_gate_v1.so"))),
            "before_code_hash":hex::encode(hash(&artifact("v4-disabled/launch_vault_v4.so"))),
            "after_code_hash":hex::encode(hash(&bytes)),"after_code_bytes":bytes.len(),
            "proposal_nonce":1,"created_at":f.g().created_at,"execute_after":maturity,
            "early_execute_rejected_at":maturity-1,"execute_succeeded_at":maturity,
            "proposal_signature":proposal_sig,"execute_signature":execute_sig,
            "signed_transactions_sent":f.sent,"successful_transactions":f.accepted,
            "one_committee_key_absent":true,"permissionless_execute":true,"private_keys_serialized":false,
            "accountable_human_audit":false
        });
        std::fs::write(
            path,
            serde_json::to_string_pretty(&receipt).unwrap()
                + "
",
        )
        .unwrap();
    }
}

#[test]
fn cancellation_unlocks_only_bound_buffer_and_reproposal_restarts_full_notice() {
    let mut f = Fixture::new(false, true);
    f.init();
    let bytes = artifact("v4-test/launch_vault_v4.so");
    let buffer = f.upload(&bytes);
    f.seal_buffer(&buffer);
    f.call(f.proposal_ix(buffer.pubkey(), 1, hash(&bytes)))
        .unwrap();
    let old_maturity = f.g().execute_after;
    assert!(f
        .call(ix(
            f.return_accounts(buffer.pubkey(), f.owner.pubkey()),
            instruction::ReturnUnqueuedBuffer {}
        ))
        .is_err());
    assert!(f
        .call(ix(
            f.return_accounts(buffer.pubkey(), f.outsider.pubkey()),
            instruction::CancelUpgrade { nonce: 1 }
        ))
        .is_err());
    f.time(START + 1);
    f.call(ix(
        f.return_accounts(buffer.pubkey(), f.owner.pubkey()),
        instruction::CancelUpgrade { nonce: 1 },
    ))
    .unwrap();
    assert_eq!(f.g().status, CANCELLED);
    f.call(loader::write(
        &buffer.pubkey(),
        &f.owner.pubkey(),
        0,
        bytes[..8].to_vec(),
    ))
    .unwrap();
    f.seal_buffer(&buffer);
    assert!(f
        .call(f.proposal_ix(buffer.pubkey(), 1, hash(&bytes)))
        .is_err());
    f.call(f.proposal_ix(buffer.pubkey(), 2, hash(&bytes)))
        .unwrap();
    assert_eq!(f.g().execute_after, old_maturity + 1);
    f.time(old_maturity);
    assert!(f.call(f.execute_ix(buffer.pubkey(), 2)).is_err());
    f.time(old_maturity + 1);
    f.call(f.execute_ix(buffer.pubkey(), 2)).unwrap();
}

#[test]
fn invalid_elf_failure_rolls_back_gate_buffer_and_programdata_atomically() {
    let mut f = Fixture::new(false, true);
    f.init();
    let bytes = vec![0; 1024];
    let buffer = f.upload(&bytes);
    f.seal_buffer(&buffer);
    f.call(f.proposal_ix(buffer.pubkey(), 1, hash(&bytes)))
        .unwrap();
    f.time(f.g().execute_after);
    let gate = f.svm.get_account(&f.gate).unwrap();
    let buffered = f.svm.get_account(&buffer.pubkey()).unwrap();
    let program = f.programdata(f.target.pubkey());
    assert!(f.call(f.execute_ix(buffer.pubkey(), 1)).is_err());
    assert_eq!(f.svm.get_account(&f.gate).unwrap(), gate);
    assert_eq!(f.svm.get_account(&buffer.pubkey()).unwrap(), buffered);
    assert_eq!(f.programdata(f.target.pubkey()), program);
    assert_eq!(f.g().status, PENDING);
}

#[test]
fn unqueued_buffer_return_and_clock_checks_do_not_bypass_queued_hash_binding() {
    let mut f = Fixture::new(false, true);
    f.init();
    let bytes = artifact("v4-test/launch_vault_v4.so");
    let buffer = f.upload(&bytes);
    f.seal_buffer(&buffer);
    let other = f.upload(&[1, 2, 3]);
    f.seal_buffer(&other);
    f.call(f.proposal_ix(buffer.pubkey(), 1, hash(&bytes)))
        .unwrap();
    f.call(ix(
        f.return_accounts(other.pubkey(), f.owner.pubkey()),
        instruction::ReturnUnqueuedBuffer {},
    ))
    .unwrap();
    f.time(START - 1);
    assert!(f.call(f.execute_ix(buffer.pubkey(), 1)).is_err());
    f.time(START + NOTICE);
    assert!(f.call(f.execute_ix(other.pubkey(), 1)).is_err());
    let mut tampered = f.svm.get_account(&buffer.pubkey()).unwrap();
    tampered.data[LoaderState::size_of_buffer_metadata() + 20] ^= 1;
    f.svm.set_account(buffer.pubkey(), tampered).unwrap(); // explicit state-corruption test, not a transaction
    assert!(f.call(f.execute_ix(buffer.pubkey(), 1)).is_err());
    assert_eq!(f.g().status, PENDING);
}
