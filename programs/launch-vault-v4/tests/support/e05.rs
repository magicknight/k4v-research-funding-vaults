//! Combined rehearsal: signed loader/SPL setup, funded state, key recovery,
//! changed-ELF upgrade, continued withdrawals and annual rollover in one bank.
//! Only Clock/slot control and SOL airdrops are fixture setup shortcuts.
use super::*;
use anchor_lang::solana_program::{bpf_loader_upgradeable::ID as LOADER, sysvar::SysvarId};
use solana_instruction::AccountMeta;
use solana_loader_v3_interface::{
    get_program_data_address, instruction as loader, state::UpgradeableLoaderState as LoaderState,
};
use solana_system_interface::instruction as system;

fn key(label: &str) -> Keypair {
    Keypair::new_from_array(solana_sha256_hasher::hash(label.as_bytes()).to_bytes())
}
fn gate_id() -> Pubkey {
    key("k4v-upgrade-gate-v1-test-only").pubkey()
}
fn gate_address() -> Pubkey {
    Pubkey::find_program_address(&[b"upgrade-gate-v1", ID.as_ref()], &gate_id()).0
}
fn artifact(path: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(path),
    )
    .unwrap()
}
fn upload(f: &mut Fixture, bytes: &[u8]) -> Keypair {
    let buffer = Keypair::new();
    f.run_extra(
        loader::create_buffer(
            &f.creator.pubkey(),
            &buffer.pubkey(),
            &f.creator.pubkey(),
            f.svm
                .minimum_balance_for_rent_exemption(LoaderState::size_of_buffer(bytes.len())),
            bytes.len(),
        )
        .unwrap(),
        f.creator.pubkey(),
        &[&buffer],
    )
    .unwrap();
    for (i, chunk) in bytes.chunks(900).enumerate() {
        f.run(loader::write(
            &buffer.pubkey(),
            &f.creator.pubkey(),
            (i * 900) as u32,
            chunk.to_vec(),
        ))
        .unwrap();
    }
    buffer
}
fn deploy(f: &mut Fixture, program: &Keypair, bytes: &[u8], seal: bool) {
    let buffer = upload(f, bytes);
    f.run_extra(
        loader::deploy_with_max_program_len(
            &f.creator.pubkey(),
            &program.pubkey(),
            &buffer.pubkey(),
            &f.creator.pubkey(),
            f.svm
                .minimum_balance_for_rent_exemption(LoaderState::size_of_program()),
            bytes.len() + 64_000,
        )
        .unwrap(),
        f.creator.pubkey(),
        &[program],
    )
    .unwrap();
    let a = f
        .svm
        .get_account(&get_program_data_address(&program.pubkey()))
        .unwrap();
    assert_eq!(&a.data[45..45 + bytes.len()], bytes);
    assert!(a.data[45 + bytes.len()..].iter().all(|b| *b == 0));
    if seal {
        f.run(loader::set_upgrade_authority(
            &program.pubkey(),
            &f.creator.pubkey(),
            None,
        ))
        .unwrap();
    }
}

// Gate ABI is encoded explicitly from the frozen E-04 interface. This test
// does not introduce a program dependency or unify its feature flags.
fn gate_ix(name: &str, metas: Vec<AccountMeta>, args: &[u8]) -> Instruction {
    let mut data =
        solana_sha256_hasher::hash(format!("global:{name}").as_bytes()).to_bytes()[..8].to_vec();
    data.extend_from_slice(args);
    Instruction {
        program_id: gate_id(),
        accounts: metas,
        data,
    }
}
fn ro(key: Pubkey, signer: bool) -> AccountMeta {
    AccountMeta::new_readonly(key, signer)
}
fn rw(key: Pubkey, signer: bool) -> AccountMeta {
    AccountMeta::new(key, signer)
}

pub(super) fn prepare(
    f: &mut Fixture,
    mint: &Keypair,
    source: &Keypair,
    founder: &Keypair,
    recipient: &Keypair,
) {
    let target = key("k4v-launch-vault-v4-recovery-test-only");
    assert_eq!(target.pubkey(), ID);
    deploy(f, &target, &artifact("v4-test/launch_vault_v4.so"), false);
    deploy(
        f,
        &key("k4v-upgrade-gate-v1-test-only"),
        &artifact("gate-test/upgrade_gate_v1.so"),
        true,
    );
    f.run_extra(
        vec![
            system::create_account(
                &f.creator.pubkey(),
                &mint.pubkey(),
                f.svm.minimum_balance_for_rent_exemption(Mint::LEN),
                Mint::LEN as u64,
                &TOKEN_ID,
            ),
            spl_token_interface::instruction::initialize_mint2(
                &TOKEN_ID,
                &mint.pubkey(),
                &f.creator.pubkey(),
                None,
                9,
            )
            .unwrap(),
        ],
        f.creator.pubkey(),
        &[mint],
    )
    .unwrap();
    for (k, owner) in [
        (source, f.depositor.pubkey()),
        (founder, f.founder.pubkey()),
        (recipient, f.outsider.pubkey()),
    ] {
        f.run_extra(
            vec![
                system::create_account(
                    &f.creator.pubkey(),
                    &k.pubkey(),
                    f.svm.minimum_balance_for_rent_exemption(SplAccount::LEN),
                    SplAccount::LEN as u64,
                    &TOKEN_ID,
                ),
                spl_token_interface::instruction::initialize_account3(
                    &TOKEN_ID,
                    &k.pubkey(),
                    &mint.pubkey(),
                    &owner,
                )
                .unwrap(),
            ],
            f.creator.pubkey(),
            &[k],
        )
        .unwrap();
    }
    f.run(
        spl_token_interface::instruction::mint_to_checked(
            &TOKEN_ID,
            &f.mint,
            &f.source,
            &f.creator.pubkey(),
            &[],
            SUPPLY,
            9,
        )
        .unwrap(),
    )
    .unwrap();
    f.run(
        spl_token_interface::instruction::set_authority(
            &TOKEN_ID,
            &f.mint,
            None,
            spl_token_interface::instruction::AuthorityType::MintTokens,
            &f.creator.pubkey(),
            &[],
        )
        .unwrap(),
    )
    .unwrap();
}

fn init_gate(f: &Fixture, members: &[Keypair; 3]) -> Instruction {
    gate_ix(
        "initialize_gate",
        vec![
            rw(f.outsider.pubkey(), true),
            ro(members[0].pubkey(), true),
            ro(members[1].pubkey(), true),
            ro(members[2].pubkey(), true),
            ro(f.creator.pubkey(), true),
            ro(ID, false),
            rw(get_program_data_address(&ID), false),
            ro(get_program_data_address(&gate_id()), false),
            rw(gate_address(), false),
            ro(LOADER, false),
            ro(solana_system_interface::program::ID, false),
        ],
        &[],
    )
}
fn propose_upgrade(
    f: &Fixture,
    members: &[Keypair; 3],
    buffer: Pubkey,
    bytes: &[u8],
) -> Instruction {
    let mut args = 1u64.to_le_bytes().to_vec();
    args.extend_from_slice(&solana_sha256_hasher::hash(bytes).to_bytes());
    gate_ix(
        "propose_upgrade",
        vec![
            ro(members[1].pubkey(), true),
            ro(members[2].pubkey(), true),
            ro(f.creator.pubkey(), true),
            rw(gate_address(), false),
            ro(buffer, false),
        ],
        &args,
    )
}
fn upgrade(f: &Fixture, buffer: Pubkey) -> Instruction {
    gate_ix(
        "execute_upgrade",
        vec![
            rw(gate_address(), false),
            rw(ID, false),
            rw(get_program_data_address(&ID), false),
            rw(buffer, false),
            rw(f.creator.pubkey(), false),
            ro(anchor_lang::prelude::Rent::id(), false),
            ro(anchor_lang::prelude::Clock::id(), false),
            ro(LOADER, false),
        ],
        &1u64.to_le_bytes(),
    )
}

fn snapshot(
    f: &Fixture,
    label: &str,
    approval_period: u64,
    buffer: Option<Pubkey>,
) -> serde_json::Value {
    let mut keys = vec![
        ("policy".to_owned(), f.policy),
        ("founder_vault".into(), f.vault(FOUNDER)),
        ("treasury_vault".into(), f.vault(TREASURY)),
        ("mint".into(), f.mint),
        ("source".into(), f.source),
        ("founder_token".into(), f.vault_token(FOUNDER)),
        ("treasury_token".into(), f.vault_token(TREASURY)),
        ("founder_destination".into(), f.founder_out),
        ("treasury_destination".into(), f.recipient),
        ("approval".into(), f.approval(approval_period)),
        ("target_program".into(), ID),
        ("target_programdata".into(), get_program_data_address(&ID)),
        ("gate_program".into(), gate_id()),
        (
            "gate_programdata".into(),
            get_program_data_address(&gate_id()),
        ),
        ("upgrade_gate".into(), gate_address()),
        ("clock".into(), Clock::id()),
    ];
    for nonce in 1..=f.p().change_sequence {
        keys.push((format!("change_{nonce}"), proposal(f, nonce)));
    }
    if let Some(b) = buffer {
        keys.push(("upgrade_buffer".into(), b));
    }
    let mut accounts = serde_json::Map::new();
    for (name, key) in keys {
        let a = f.svm.get_account(&key).unwrap();
        accounts.insert(
            name,
            serde_json::json!({"address":key.to_string(), "owner":a.owner.to_string(),
            "executable":a.executable, "data_hex":hex::encode(a.data)}),
        );
    }
    serde_json::json!({"schema":"K4V-LAUNCH-V4-RAW-SNAPSHOT-v1", "scope":"AUTHOR_RUN_LOCAL_LITESVM",
        "program_id":ID.to_string(), "now":f.svm.get_sysvar::<Clock>().unix_timestamp.to_string(),
        "private_keys_serialized":false, "label":label, "accounts":accounts})
}

#[test]
fn full_funding_recovery_changed_elf_upgrade_and_annual_rehearsal() {
    let mut f = Fixture::new_mode(false, false, true);
    let members = [Keypair::new(), Keypair::new(), Keypair::new()];
    f.run_extra(
        vec![init_gate(&f, &members)],
        f.outsider.pubkey(),
        &[&members[0], &members[1], &members[2]],
    )
    .unwrap();
    f.active();
    f.run(f.approve_ix(6, f.config.treasury_period_cap, f.recipient))
        .unwrap();
    f.time(f.config.t0 + 3 * PERIOD);
    let now = f.svm.get_sysvar::<Clock>().unix_timestamp;
    f.run_many(
        vec![change(&f, CHANGE_ORACLE, true, 1, f.outsider.pubkey())],
        f.outsider.pubkey(),
    )
    .unwrap();
    assert!(!f.last_signers.contains(&f.creator.pubkey()));
    let oracle_proposal_signature = f.last_signature.clone();
    let replacement = artifact("v4-disabled/launch_vault_v4.so");
    let buffer = upload(&mut f, &replacement);
    f.run(loader::set_buffer_authority(
        &buffer.pubkey(),
        &f.creator.pubkey(),
        &gate_address(),
    ))
    .unwrap();
    f.run_extra(
        vec![propose_upgrade(&f, &members, buffer.pubkey(), &replacement)],
        f.outsider.pubkey(),
        &[&members[1], &members[2]],
    )
    .unwrap();
    let upgrade_proposal_signature = f.last_signature.clone();
    let mut checkpoints = vec![snapshot(
        &f,
        "pending_oracle_upgrade",
        6,
        Some(buffer.pubkey()),
    )];
    let maturity = now + CHANGE_NOTICE;
    f.time(maturity - 1);
    f.reject_unchanged(execute(&f, 1), "ChangeNotice");
    f.reject_unchanged(upgrade(&f, buffer.pubkey()), "Notice");
    f.report(2_000_000 * UNIT);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "CliffActive");
    checkpoints.push(snapshot(&f, "notice_minus_one", 6, Some(buffer.pubkey())));
    f.time(maturity);
    f.report(2_000_000 * UNIT);
    f.run(f.release_ix(FOUNDER, 200_000 * UNIT, 0)).unwrap();
    f.run(f.release_ix(TREASURY, 300_000 * UNIT, 6)).unwrap();
    checkpoints.push(snapshot(
        &f,
        "before_oracle_recovery",
        6,
        Some(buffer.pubkey()),
    ));
    let before = counters(&f.p());
    f.run_many(vec![execute(&f, 1)], f.outsider.pubkey())
        .unwrap();
    assert_eq!(f.last_signers, vec![f.outsider.pubkey()]);
    assert_eq!(before, counters(&f.p()));
    let oracle_execute_signature = f.last_signature.clone();
    checkpoints.push(snapshot(
        &f,
        "after_oracle_recovery",
        6,
        Some(buffer.pubkey()),
    ));
    f.reject_unchanged(
        epoch_report(&f, f.oracle.pubkey(), 0, 2, maturity),
        "ConstraintHasOne",
    );
    f.reject_unchanged(
        epoch_report(&f, f.outsider.pubkey(), 0, 2, maturity),
        "InvalidReport",
    );
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 0), "InvalidReport");
    f.run(epoch_report(&f, f.outsider.pubkey(), 1, 3, maturity))
        .unwrap();
    f.run(f.release_ix(FOUNDER, 100_000 * UNIT, 0)).unwrap();
    f.run(f.release_ix(TREASURY, 150_000 * UNIT, 6)).unwrap();
    checkpoints.push(snapshot(&f, "before_upgrade", 6, Some(buffer.pubkey())));
    let before = counters(&f.p());
    f.run_many(vec![upgrade(&f, buffer.pubkey())], f.outsider.pubkey())
        .unwrap();
    assert_eq!(f.last_signers, vec![f.outsider.pubkey()]);
    assert_eq!(before, counters(&f.p()));
    let upgrade_execute_signature = f.last_signature.clone();
    checkpoints.push(snapshot(&f, "after_upgrade", 6, None));
    f.reject_unchanged(upgrade(&f, buffer.pubkey()), "InvalidProposal");
    f.run(epoch_report(&f, f.outsider.pubkey(), 1, 4, maturity))
        .unwrap();
    f.run(f.release_ix(FOUNDER, 100_000 * UNIT, 0)).unwrap();
    f.run(f.release_ix(TREASURY, 150_000 * UNIT, 6)).unwrap();
    f.conserved();
    checkpoints.push(snapshot(&f, "continued_after_upgrade", 6, None));
    f.run(f.approve_ix(9, f.config.treasury_period_cap, f.recipient))
        .unwrap();
    f.run_many(
        vec![change(
            &f,
            CHANGE_CONTROLLER,
            true,
            2,
            f.recovery[2].pubkey(),
        )],
        f.outsider.pubkey(),
    )
    .unwrap();
    assert!(!f.last_signers.contains(&f.creator.pubkey()));
    f.time(maturity + CHANGE_NOTICE - 1);
    f.reject_unchanged(execute(&f, 2), "ChangeNotice");
    checkpoints.push(snapshot(&f, "controller_notice_minus_one", 9, None));
    f.time(maturity + CHANGE_NOTICE);
    f.run(epoch_report(
        &f,
        f.outsider.pubkey(),
        1,
        5,
        maturity + CHANGE_NOTICE,
    ))
    .unwrap();
    checkpoints.push(snapshot(&f, "before_controller_recovery", 9, None));
    f.run_many(vec![execute(&f, 2)], f.outsider.pubkey())
        .unwrap();
    checkpoints.push(snapshot(&f, "after_controller_recovery", 9, None));
    let mut old_controller = change(&f, CHANGE_CONTROLLER, false, 3, f.founder.pubkey());
    old_controller.accounts[1].pubkey = f.creator.pubkey();
    old_controller.accounts[2].pubkey = f.creator.pubkey();
    f.reject_unchanged(old_controller, "Unauthorized");
    f.reject_unchanged(execute(&f, 1), "InvalidChange");
    f.reject_unchanged(execute(&f, 2), "InvalidChange");
    f.run(change(&f, CHANGE_CONTROLLER, false, 3, f.creator.pubkey()))
        .unwrap();
    f.run(cancel_change(
        &f,
        3,
        f.recovery[2].pubkey(),
        f.recovery[2].pubkey(),
    ))
    .unwrap();
    f.reject_unchanged(execute(&f, 3), "InvalidChange");
    f.run(f.release_ix(FOUNDER, 100_000 * UNIT, 0)).unwrap();
    f.run(f.release_ix(TREASURY, 150_000 * UNIT, 9)).unwrap();
    f.conserved();
    checkpoints.push(snapshot(&f, "continued_after_controller_recovery", 9, None));
    f.run(f.approve_ix(12, f.config.treasury_period_cap, f.recipient))
        .unwrap();
    f.time(f.config.t0 + 12 * PERIOD);
    let year = f.svm.get_sysvar::<Clock>().unix_timestamp;
    f.run(epoch_report(&f, f.outsider.pubkey(), 1, 6, year))
        .unwrap();
    f.run(f.release_ix(FOUNDER, 100_000 * UNIT, 0)).unwrap();
    f.run(f.release_ix(TREASURY, 150_000 * UNIT, 12)).unwrap();
    assert_eq!(f.p().founder_released_total, 600_000 * UNIT);
    assert_eq!(f.p().treasury_released_total, 900_000 * UNIT);
    assert_eq!(f.p().founder_annual_used, 100_000 * UNIT);
    assert_eq!(f.p().treasury_annual_used, 150_000 * UNIT);
    f.conserved();
    checkpoints.push(snapshot(&f, "annual_boundary", 12, None));
    if let Ok(path) = std::env::var("K4V_E05_REHEARSAL_OUT") {
        let receipt = serde_json::json!({"schema":"K4V-E05-RAW-REHEARSAL-v1", "checkpoints":checkpoints,
            "environment":"LiteSVM native loader and signed SPL transactions; controlled Clock/slots and SOL airdrops",
            "program_injection_used":false, "mint_or_token_account_injection_used":false,
            "public_chain_transactions":0, "private_keys_serialized":false,
            "signed_transactions_sent":f.sent, "successful_transactions":f.accepted,
            "oracle_proposal_signature":oracle_proposal_signature, "oracle_execute_signature":oracle_execute_signature,
            "upgrade_proposal_signature":upgrade_proposal_signature, "upgrade_execute_signature":upgrade_execute_signature,
            "independent_human_audit":false});
        std::fs::write(path, serde_json::to_string(&receipt).unwrap() + "\n").unwrap();
    }
}
