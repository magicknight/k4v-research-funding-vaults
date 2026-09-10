//! E-10 signed native-loader/SPL setup and beneficiary recovery continuity.
//! Clock/slot control and SOL airdrops remain local fixture setup.
use super::*;
use anchor_lang::solana_program::{bpf_loader_upgradeable::ID as LOADER, sysvar::SysvarId};

use solana_loader_v3_interface::{
    get_program_data_address, instruction as loader, state::UpgradeableLoaderState as LoaderState,
};
use solana_system_interface::instruction as system;

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

pub(super) fn prepare(
    f: &mut Fixture,
    mint: &Keypair,
    source: &Keypair,
    founder: &Keypair,
    recipient: &Keypair,
) {
    let target = Keypair::new_from_array([88; 32]);
    assert_eq!(target.pubkey(), ID);
    deploy(f, &target, &artifact("v6-test/launch_vault_v6.so"), true);
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

fn create_token(f: &mut Fixture, token: &Keypair, owner: Pubkey) {
    f.run_extra(
        vec![
            system::create_account(
                &f.creator.pubkey(),
                &token.pubkey(),
                f.svm.minimum_balance_for_rent_exemption(SplAccount::LEN),
                SplAccount::LEN as u64,
                &TOKEN_ID,
            ),
            spl_token_interface::instruction::initialize_account3(
                &TOKEN_ID,
                &token.pubkey(),
                &f.mint,
                &owner,
            )
            .unwrap(),
        ],
        f.creator.pubkey(),
        &[token],
    )
    .unwrap();
}

fn snapshot(
    f: &Fixture,
    label: &str,
    old_destination: Pubkey,
    new_destination: Pubkey,
    known_keys: &[Pubkey],
) -> serde_json::Value {
    let mut addresses = vec![
        ("policy".to_string(), f.policy),
        ("mint".into(), f.mint),
        ("source".into(), f.source),
        ("founder_vault".into(), f.vault(FOUNDER)),
        ("treasury_vault".into(), f.vault(TREASURY)),
        ("founder_token".into(), f.vault_token(FOUNDER)),
        ("treasury_token".into(), f.vault_token(TREASURY)),
        ("founder_destination_0".into(), old_destination),
        ("founder_destination_1".into(), new_destination),
        ("treasury_destination".into(), f.recipient),
        ("program".into(), ID),
        ("program_data".into(), get_program_data_address(&ID)),
        ("clock".into(), Clock::id()),
    ];
    for period in [6, 9, 13] {
        addresses.push((format!("approval_{period}"), f.approval(period)));
    }
    for subject in known_keys {
        let address = f.key_record(*subject);
        if f.svm.get_account(&address).is_some() {
            addresses.push((format!("key_{subject}"), address));
        }
    }
    for role in [FOUNDER, TREASURY] {
        for n in 1..=f.p().withdrawal[role as usize].sequence {
            addresses.push((
                format!("withdrawal_{role}_{n}"),
                f.withdrawal_proposal(role, n),
            ));
        }
    }
    let accounts: serde_json::Map<String, serde_json::Value> = addresses.into_iter().map(|(name, address)| {
        let a = f.svm.get_account(&address).unwrap();
        (name, serde_json::json!({"address":address.to_string(), "owner":a.owner.to_string(),
            "executable":a.executable,"lamports":a.lamports.to_string(),"data_hex":hex::encode(a.data)}))
    }).collect();
    let clock = f.svm.get_sysvar::<Clock>();
    serde_json::json!({"schema":"K4V-LAUNCH-V6-RAW-SNAPSHOT-v1","program_id":ID.to_string(),
        "scope":"AUTHOR_RUN_LOCAL_LITESVM","private_keys_serialized":false,
        "label":label,"now":clock.unix_timestamp.to_string(),"slot":clock.slot.to_string(),
        "last_local_signature":f.last_signature,"accounts":accounts})
}

fn balances(f: &Fixture, old: Pubkey, new: Pubkey) {
    let sum: u128 = [
        f.source,
        old,
        new,
        f.recipient,
        f.vault_token(FOUNDER),
        f.vault_token(TREASURY),
    ]
    .iter()
    .map(|k| u128::from(f.balance(*k)))
    .sum();
    assert_eq!(sum, u128::from(SUPPLY));
}

#[test]
fn signed_native_loader_dual_withdrawal_recovery_preserves_money_notices_and_year_boundary() {
    let mut f = Fixture::new_mode(false, false, true);
    let new_f = key("e10-founder-successor-symbolic-test-key");
    let new_t = key("e10-treasury-successor-symbolic-test-key");
    let spare = clone_key(&f.depositor);
    let new_token = Keypair::new();
    create_token(&mut f, &new_token, new_f.pubkey());
    let old_destination = f.founder_out;
    let keys = [
        f.founder.pubkey(),
        f.treasury.pubkey(),
        f.outsider.pubkey(),
        new_f.pubkey(),
        new_t.pubkey(),
        spare.pubkey(),
    ];
    f.fund();
    for period in [6, 9, 13] {
        f.run(f.approve_ix(period, 300_000 * UNIT, f.recipient))
            .unwrap();
    }
    f.run(f.arm_ix()).unwrap();
    f.time(f.config.t0);
    f.run(f.activate_ix()).unwrap();
    f.time(f.config.t0 + CLIFF);
    f.report(f.config.shared_hard_cap);
    f.run(f.release_ix(FOUNDER, 100_000 * UNIT, 6)).unwrap();
    f.run(f.release_ix(TREASURY, 150_000 * UNIT, 6)).unwrap();
    let mut snapshots = vec![snapshot(
        &f,
        "partial_before",
        old_destination,
        new_token.pubkey(),
        &keys,
    )];
    let mut delayed_submissions = vec![];
    for (role, successor) in [(FOUNDER, &new_f), (TREASURY, &new_t)] {
        f.prepare_key(successor.pubkey());
        let signed_at = f.svm.get_sysvar::<Clock>().unix_timestamp;
        let instruction = f.propose_withdrawal_ix(role, successor.pubkey(), true);
        f.run_extra_delayed(vec![instruction], f.outsider.pubkey(), &[successor], 30)
            .unwrap();
        let q: WithdrawalProposalV6 = f.read(f.withdrawal_proposal(role, 1));
        assert_eq!(
            (q.valid_from, q.valid_until, q.created_at),
            (signed_at, signed_at + 300, signed_at + 30)
        );
        assert_eq!(q.execute_after, q.created_at + CHANGE_NOTICE);
        delayed_submissions.push(serde_json::json!({"role":role,"delay_seconds":30,
            "valid_from":q.valid_from,"valid_until":q.valid_until,"accepted_at":q.created_at,
            "local_signature":f.last_signature,"same_valid_blockhash":true}));
    }
    snapshots.push(snapshot(
        &f,
        "recovery_pending",
        old_destination,
        new_token.pubkey(),
        &keys,
    ));
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 6), "WithdrawalPaused");
    f.reject_unchanged(f.release_ix(TREASURY, 1, 6), "WithdrawalPaused");
    f.reject_unchanged(f.approve_ix(14, UNIT, f.recipient), "WithdrawalPaused");
    f.time(
        f.read::<WithdrawalProposalV6>(f.withdrawal_proposal(FOUNDER, 1))
            .execute_after
            - 1,
    );
    f.reject_unchanged(
        f.execute_withdrawal_ix(FOUNDER, 1, false),
        "WithdrawalWindow",
    );
    snapshots.push(snapshot(
        &f,
        "notice_minus_one",
        old_destination,
        new_token.pubkey(),
        &keys,
    ));
    f.time(
        f.read::<WithdrawalProposalV6>(f.withdrawal_proposal(TREASURY, 1))
            .execute_after,
    );
    f.report(f.config.shared_hard_cap);
    snapshots.push(snapshot(
        &f,
        "before_execute",
        old_destination,
        new_token.pubkey(),
        &keys,
    ));
    let original_counters = counters(&f.p());
    let vaults = [FOUNDER, TREASURY].map(|r| f.svm.get_account(&f.vault(r)).unwrap().data);
    let approvals = [6, 9, 13].map(|p| f.svm.get_account(&f.approval(p)).unwrap().data);
    let old_f = clone_key(&f.founder);
    let old_t = clone_key(&f.treasury);
    for role in [FOUNDER, TREASURY] {
        f.run_many(
            vec![f.execute_withdrawal_ix(role, 1, false)],
            f.outsider.pubkey(),
        )
        .unwrap();
        assert_eq!(f.last_signers, vec![f.outsider.pubkey()]);
        assert_eq!(counters(&f.p()), original_counters);
        assert_eq!(
            [FOUNDER, TREASURY].map(|r| f.svm.get_account(&f.vault(r)).unwrap().data),
            vaults
        );
        assert_eq!(
            [6, 9, 13].map(|p| f.svm.get_account(&f.approval(p)).unwrap().data),
            approvals
        );
        snapshots.push(snapshot(
            &f,
            if role == FOUNDER {
                "after_founder"
            } else {
                "after_treasury"
            },
            old_destination,
            new_token.pubkey(),
            &keys,
        ));
    }
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 9), "WithdrawalAuthority");
    f.reject_unchanged(f.release_ix(TREASURY, 1, 9), "WithdrawalAuthority");
    f.founder = clone_key(&new_f);
    f.treasury = clone_key(&new_t);
    f.founder_out = new_token.pubkey();
    // A new operating key needs SOL for approval rent; no new token entitlement.
    f.svm.airdrop(&new_t.pubkey(), 1_000_000_000).unwrap();
    let mut stale_epoch = f.release_ix(FOUNDER, UNIT, 9);
    stale_epoch.data[16..24].copy_from_slice(&0u64.to_le_bytes());
    f.reject_unchanged(stale_epoch, "WithdrawalAuthority");
    let mut wrong_destination = f.release_ix(FOUNDER, UNIT, 9);
    wrong_destination.accounts[5].pubkey = old_destination;
    f.reject_unchanged(wrong_destination, "Unauthorized");
    f.run(f.release_ix(FOUNDER, 100_000 * UNIT, 9)).unwrap();
    f.run(f.release_ix(TREASURY, 150_000 * UNIT, 9)).unwrap();
    assert_eq!(
        f.read::<TreasuryApprovalV6>(f.approval(9)).author,
        old_t.pubkey()
    );
    f.reject_unchanged(
        f.release_ix(TREASURY, 150_000 * UNIT + 1, 9),
        "InvalidApproval",
    );
    f.reject_unchanged(
        f.release_ix(FOUNDER, 900_000 * UNIT + 1, 9),
        "ReservedQuotaExceeded",
    );
    snapshots.push(snapshot(
        &f,
        "continued_period_9",
        old_destination,
        new_token.pubkey(),
        &keys,
    ));
    // Accepted normal cancellation and expiry retain spent nonces and key indexes.
    f.propose_withdrawal(FOUNDER, &spare, false);
    f.run(f.cancel_withdrawal_ix(FOUNDER, 2, new_f.pubkey(), new_f.pubkey()))
        .unwrap();
    snapshots.push(snapshot(
        &f,
        "normal_cancelled",
        old_destination,
        new_token.pubkey(),
        &keys,
    ));
    f.propose_withdrawal(FOUNDER, &spare, false);
    snapshots.push(snapshot(
        &f,
        "normal_pending_expiry",
        old_destination,
        new_token.pubkey(),
        &keys,
    ));
    let expiry = f
        .read::<WithdrawalProposalV6>(f.withdrawal_proposal(FOUNDER, 3))
        .expires_at;
    assert_eq!(expiry, f.config.t0 + 13 * PERIOD + 60);
    f.time(expiry);
    f.reject_unchanged(
        f.execute_withdrawal_ix(FOUNDER, 3, false),
        "WithdrawalWindow",
    );
    f.run_many(
        vec![f.execute_withdrawal_ix(FOUNDER, 3, true)],
        f.outsider.pubkey(),
    )
    .unwrap();
    snapshots.push(snapshot(
        &f,
        "expired",
        old_destination,
        new_token.pubkey(),
        &keys,
    ));
    f.report(f.config.shared_hard_cap);
    f.run(f.release_ix(FOUNDER, 100_000 * UNIT, 13)).unwrap();
    f.run(f.release_ix(TREASURY, 150_000 * UNIT, 13)).unwrap();
    assert_eq!(
        (f.p().founder_annual_used, f.p().treasury_annual_used),
        (100_000 * UNIT, 150_000 * UNIT)
    );
    assert_eq!(
        (f.p().founder_released_total, f.p().treasury_released_total),
        (300_000 * UNIT, 450_000 * UNIT)
    );
    assert_eq!(f.p().founder, old_f.pubkey());
    snapshots.push(snapshot(
        &f,
        "continued_year_two",
        old_destination,
        new_token.pubkey(),
        &keys,
    ));
    balances(&f, old_destination, new_token.pubkey());
    let data = f.svm.get_account(&get_program_data_address(&ID)).unwrap();
    assert_eq!(data.owner, LOADER);
    assert_eq!(data.data[12], 0);
    if let Ok(path) = std::env::var("K4V_E10_REHEARSAL_OUT") {
        let value = serde_json::json!({"schema":"K4V-E10-LOCAL-REHEARSAL-v1",
            "scope":"AUTHOR_RUN_LOCAL_LITESVM","private_keys_serialized":false,
            "signed_transactions_sent":f.sent,"signed_transactions_successful":f.accepted,
            "program_or_token_injection":false,"public_chain_transactions":0,
            "clock_controlled":true,"fee_airdrops":true,
            "delayed_submissions":delayed_submissions,"snapshots":snapshots});
        std::fs::write(path, serde_json::to_string_pretty(&value).unwrap() + "\n").unwrap();
    }
}

fn clone_key(k: &Keypair) -> Keypair {
    Keypair::from_base58_string(&k.to_base58_string())
}
