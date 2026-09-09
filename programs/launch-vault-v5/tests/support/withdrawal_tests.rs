use super::*;

fn clone_key(k: &Keypair) -> Keypair {
    Keypair::from_base58_string(&k.to_base58_string())
}

#[test]
fn beneficiary_registration_binds_each_committee_and_rejects_bad_backups() {
    for role in 0..2 {
        let mut f = Fixture::new(false, false);
        let keys = if role == 0 {
            &mut f.config.founder_recovery_keys
        } else {
            &mut f.config.treasury_recovery_keys
        };
        keys[0] = f.outsider.pubkey();
        f.reject_unchanged(f.open_ix(), "IdentityMismatch");
        f.rebind();
        f.run(f.open_ix()).unwrap();
    }
    for invalid in 0..3 {
        let mut f = Fixture::new(false, false);
        f.config.founder_recovery_keys[0] = match invalid {
            0 => f.config.founder_recovery_keys[1],
            1 => Pubkey::default(),
            _ => f.founder.pubkey(),
        };
        f.rebind();
        f.reject_unchanged(f.open_ix(), "InvalidConfig");
    }
}

#[test]
fn key_indexes_are_canonical_permanent_and_prefunding_does_not_steal_them() {
    let mut f = Fixture::new(false, false);
    f.fund();
    let subject = f.backups[0][2].pubkey();
    let address = f.key_record(subject);
    f.run(solana_system_interface::instruction::transfer(
        &f.creator.pubkey(),
        &address,
        1,
    ))
    .unwrap();
    f.run(f.prepare_key_ix(subject)).unwrap();
    let record: WithdrawalKeyV5 = f.read(address);
    assert_eq!(record.subject, subject);
    assert_eq!(
        (record.history_mask, record.pending_mask, record.recipient),
        (0, 0, false)
    );
    f.reject_unchanged(f.prepare_key_ix(subject), "already in use");
    let mut substituted = f.prepare_key_ix(f.oracle.pubkey());
    substituted.accounts[2].pubkey = f.key_record(f.outsider.pubkey());
    f.reject_unchanged(substituted, "ConstraintSeeds");
}

#[test]
fn recovery_rejects_single_duplicate_controller_or_other_role_quorum_and_missing_acceptance() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    let new = clone_key(&f.depositor);
    f.prepare_key(new.pubkey());
    for pair in [
        [f.backups[0][0].pubkey(); 2],
        [f.creator.pubkey(); 2],
        [f.backups[1][0].pubkey(), f.backups[1][1].pubkey()],
    ] {
        let mut i = f.propose_withdrawal_ix(FOUNDER, new.pubkey(), true);
        i.accounts[1].pubkey = pair[0];
        i.accounts[2].pubkey = pair[1];
        f.reject_unchanged(i, "RecoveryQuorum");
    }
    let mut i = f.propose_withdrawal_ix(FOUNDER, new.pubkey(), true);
    i.accounts[3].is_signer = false;
    f.reject_unchanged(i, "AccountNotSigner");
    let mut i = f.propose_withdrawal_ix(FOUNDER, new.pubkey(), false);
    i.accounts[1].pubkey = f.creator.pubkey();
    i.accounts[2].pubkey = f.creator.pubkey();
    f.reject_unchanged(i, "WithdrawalAuthority");
    f.propose_withdrawal(FOUNDER, &new, true);
    assert!(!f.last_signers.contains(&f.founder.pubkey()));
    assert!(!f.last_signers.contains(&f.creator.pubkey()));
}

#[test]
fn proposal_signature_binds_clock_nonce_epoch_and_canonical_role_accounts() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    let new = clone_key(&f.depositor);
    f.prepare_key(new.pubkey());
    let good = f.propose_withdrawal_ix(FOUNDER, new.pubkey(), true);
    for (offset, value, error) in [
        (10, 0u64, "ConstraintSeeds"),
        (18, 1, "InvalidChange"),
        (26, (f.config.t0 + CLIFF - 1) as u64, "ProposalClock"),
    ] {
        let mut i = good.clone();
        i.data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        f.reject_unchanged(i, error);
    }
    let mut i = good.clone();
    i.accounts[6].pubkey = f.key_record(f.outsider.pubkey());
    f.reject_unchanged(i, "ConstraintSeeds");
    f.propose_withdrawal(FOUNDER, &new, true);
    f.reject_unchanged(
        f.propose_withdrawal_ix(FOUNDER, f.outsider.pubkey(), true),
        "InvalidChange",
    );
}

#[test]
fn notice_cancel_expiry_and_replay_leave_financial_state_unchanged() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    let new = clone_key(&f.depositor);
    let balance = f.balance(f.vault_token(FOUNDER));
    f.propose_withdrawal(FOUNDER, &new, true);
    let q: WithdrawalProposalV5 = f.read(f.withdrawal_proposal(FOUNDER, 1));
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 6), "WithdrawalPaused");
    f.reject_unchanged(
        f.cancel_withdrawal_ix(FOUNDER, 1, f.founder.pubkey(), f.founder.pubkey()),
        "WithdrawalAuthority",
    );
    f.time(q.execute_after - 1);
    f.reject_unchanged(
        f.execute_withdrawal_ix(FOUNDER, 1, false),
        "WithdrawalWindow",
    );
    f.time(q.expires_at - 1);
    f.reject_unchanged(
        f.execute_withdrawal_ix(FOUNDER, 1, true),
        "WithdrawalWindow",
    );
    f.time(q.expires_at);
    f.reject_unchanged(
        f.execute_withdrawal_ix(FOUNDER, 1, false),
        "WithdrawalWindow",
    );
    f.run_many(
        vec![f.execute_withdrawal_ix(FOUNDER, 1, true)],
        f.outsider.pubkey(),
    )
    .unwrap();
    assert_eq!(f.p().withdrawal[0].epoch, 0);
    f.propose_withdrawal(FOUNDER, &new, false);
    let next: WithdrawalProposalV5 = f.read(f.withdrawal_proposal(FOUNDER, 2));
    assert_eq!(next.execute_after, q.expires_at + CHANGE_NOTICE);
    f.reject_unchanged(f.execute_withdrawal_ix(FOUNDER, 1, false), "InvalidChange");
    f.run(f.cancel_withdrawal_ix(FOUNDER, 2, f.founder.pubkey(), f.founder.pubkey()))
        .unwrap();
    assert_eq!(f.balance(f.vault_token(FOUNDER)), balance);
    assert_eq!(f.v(FOUNDER).released_total, 0);
    f.conserved();
}

#[test]
fn successor_or_role_quorum_can_cancel_but_other_roles_cannot() {
    for use_quorum in [false, true] {
        let mut f = Fixture::new(false, false);
        f.at_cliff();
        let new = clone_key(&f.depositor);
        f.propose_withdrawal(TREASURY, &new, true);
        f.reject_unchanged(
            f.cancel_withdrawal_ix(TREASURY, 1, f.creator.pubkey(), f.creator.pubkey()),
            "WithdrawalAuthority",
        );
        let a = if use_quorum {
            f.backups[1][1].pubkey()
        } else {
            new.pubkey()
        };
        let b = if use_quorum {
            f.backups[1][2].pubkey()
        } else {
            new.pubkey()
        };
        f.run(f.cancel_withdrawal_ix(TREASURY, 1, a, b)).unwrap();
        let k: WithdrawalKeyV5 = f.read(f.key_record(new.pubkey()));
        assert_eq!((k.pending_mask, k.history_mask), (0, 0));
        f.reject_unchanged(f.execute_withdrawal_ix(TREASURY, 1, false), "InvalidChange");
    }
}

#[test]
fn solo_operating_key_loss_restores_roles_without_old_key_or_controller_signature() {
    let mut f = Fixture::new(false, true);
    f.at_cliff();
    let old = f.creator.pubkey();
    let new_f = Keypair::new();
    let new_t = Keypair::new();
    f.propose_withdrawal(FOUNDER, &new_f, true);
    assert!(!f.last_signers.contains(&old));
    f.propose_withdrawal(TREASURY, &new_t, true);
    assert!(!f.last_signers.contains(&old));
    f.time(f.config.t0 + CLIFF + CHANGE_NOTICE);
    for role in [FOUNDER, TREASURY] {
        f.run_many(
            vec![f.execute_withdrawal_ix(role, 1, false)],
            f.outsider.pubkey(),
        )
        .unwrap();
        assert!(!f.last_signers.contains(&old));
    }
    assert_eq!(f.p().controller, old);
    assert_eq!(f.p().founder, old);
    assert_eq!(f.p().treasury, old);
    assert_ne!(f.vault(FOUNDER), f.vault(TREASURY));
    assert_eq!(f.p().withdrawal[0].current, new_f.pubkey());
    assert_eq!(f.p().withdrawal[1].current, new_t.pubkey());
    f.report(f.config.shared_hard_cap);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 9), "WithdrawalAuthority");
    f.conserved();
}

#[test]
fn prepared_recovery_preserves_original_vault_identity_and_current_cancellation_consent() {
    let mut f = Fixture::new(false, false);
    f.config.t0 = START + 6 * PERIOD;
    f.rebind();
    f.run(f.open_ix()).unwrap();
    let original = f.founder.pubkey();
    let new = clone_key(&f.depositor);
    f.propose_withdrawal(FOUNDER, &new, true);
    f.reject_unchanged(
        f.deposit_ix(FOUNDER, f.config.founder_amount),
        "WithdrawalPaused",
    );
    f.time(START + CHANGE_NOTICE);
    f.run(f.execute_withdrawal_ix(FOUNDER, 1, false)).unwrap();
    f.reject_unchanged(f.cancel_ix(), "Unauthorized");
    f.founder = new;
    f.run(f.deposit_ix(FOUNDER, f.config.founder_amount))
        .unwrap();
    f.run(f.deposit_ix(TREASURY, f.config.treasury_amount))
        .unwrap();
    assert_eq!(f.v(FOUNDER).authority, original);
    f.run(f.arm_ix()).unwrap();
    f.run(f.cancel_ix()).unwrap();
    f.run(f.refund_ix(FOUNDER, f.source)).unwrap();
    f.run(f.refund_ix(TREASURY, f.source)).unwrap();
    f.conserved();
}

#[test]
fn armed_pending_recovery_cannot_block_consented_cancellation_and_original_refunds() {
    let mut f = Fixture::new(false, false);
    f.fund();
    f.run(f.arm_ix()).unwrap();
    let new = clone_key(&f.depositor);
    f.propose_withdrawal(FOUNDER, &new, true);
    f.run(f.cancel_ix()).unwrap();
    f.time(START + CHANGE_NOTICE);
    f.reject_unchanged(f.execute_withdrawal_ix(FOUNDER, 1, false), "InvalidState");
    f.run(f.refund_ix(FOUNDER, f.source)).unwrap();
    f.run(f.refund_ix(TREASURY, f.source)).unwrap();
    f.run(f.cancel_withdrawal_ix(FOUNDER, 1, new.pubkey(), new.pubkey()))
        .unwrap();
    f.reject_unchanged(
        f.propose_withdrawal_ix(FOUNDER, new.pubkey(), true),
        "InvalidState",
    );
    f.conserved();
}

#[test]
fn partial_unarmed_expiry_and_refund_work_during_beneficiary_pause() {
    let mut f = Fixture::new(false, false);
    f.run(f.open_ix()).unwrap();
    f.run(f.deposit_ix(FOUNDER, f.config.founder_amount))
        .unwrap();
    let new = clone_key(&f.depositor);
    f.propose_withdrawal(FOUNDER, &new, true);
    f.time(f.config.t0);
    f.run(f.expire_ix()).unwrap();
    f.run(f.refund_ix(FOUNDER, f.source)).unwrap();
    f.conserved();
}

#[test]
fn known_recipients_and_retired_keys_cannot_launder_treasury_self_payment() {
    let mut f = Fixture::new(false, false);
    f.fund();
    f.run(f.approve_ix(6, 100 * UNIT, f.recipient)).unwrap();
    let recipient = clone_key(&f.outsider);
    f.reject_unchanged(
        f.propose_withdrawal_ix(FOUNDER, recipient.pubkey(), true),
        "IneligibleSuccessor",
    );
    let guardian = f.backups[0][0].pubkey();
    f.prepare_key(guardian);
    f.reject_unchanged(
        f.propose_withdrawal_ix(FOUNDER, guardian, true),
        "IneligibleSuccessor",
    );
    f.reject_unchanged(
        f.approve_ix(7, 100 * UNIT, f.founder_out),
        "KnownSelfPayment",
    );
    let new = clone_key(&f.depositor);
    f.propose_withdrawal(FOUNDER, &new, true);
    f.reject_unchanged(f.approve_ix(7, 100 * UNIT, f.source), "KnownSelfPayment");
    f.time(START + CHANGE_NOTICE);
    f.run(f.execute_withdrawal_ix(FOUNDER, 1, false)).unwrap();
    f.reject_unchanged(f.approve_ix(7, 100 * UNIT, f.source), "KnownSelfPayment");
    f.reject_unchanged(
        f.propose_withdrawal_ix(FOUNDER, new.pubkey(), true),
        "IneligibleSuccessor",
    );
    f.reject_unchanged(
        f.propose_withdrawal_ix(FOUNDER, f.founder.pubkey(), true),
        "IneligibleSuccessor",
    );
}

#[test]
fn treasury_release_needs_original_approval_and_canonical_recipient_index() {
    let mut f = Fixture::new(false, false);
    f.fund();
    f.run(f.approve_ix(6, 100 * UNIT, f.recipient)).unwrap();
    f.run(f.arm_ix()).unwrap();
    f.time(f.config.t0 + CLIFF);
    f.run(f.activate_ix()).unwrap();
    f.report(f.config.shared_hard_cap);
    let mut missing = f.release_ix(TREASURY, UNIT, 6);
    missing.accounts[8].pubkey = ID;
    f.reject_unchanged(missing, "InvalidApproval");
    let mut swapped = f.release_ix(TREASURY, UNIT, 6);
    swapped.accounts[8].pubkey = f.key_record(f.founder.pubkey());
    f.reject_unchanged(swapped, "InvalidApproval");
    f.run(f.release_ix(TREASURY, UNIT, 6)).unwrap();
    f.conserved();
}

#[test]
fn recovery_before_cliff_cannot_accelerate_180_days_or_enlarge_first_quota() {
    let mut f = Fixture::new(false, false);
    f.active();
    let new = clone_key(&f.depositor);
    let destination = Pubkey::new_unique();
    f.svm
        .set_account(destination, token_account(f.mint, new.pubkey(), 0))
        .unwrap();
    f.propose_withdrawal(FOUNDER, &new, true);
    f.time(f.config.t0 + CHANGE_NOTICE);
    f.run(f.execute_withdrawal_ix(FOUNDER, 1, false)).unwrap();
    f.founder = new;
    f.founder_out = destination;
    f.report(f.config.shared_hard_cap);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 3), "CliffActive");
    f.time(f.config.t0 + CLIFF - 1);
    f.report(f.config.shared_hard_cap);
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 5), "CliffActive");
    f.time(f.config.t0 + CLIFF);
    f.report(f.config.shared_hard_cap);
    f.reject_unchanged(
        f.release_ix(FOUNDER, 1_000_000 * UNIT + 1, 6),
        "ReservedQuotaExceeded",
    );
    f.run(f.release_ix(FOUNDER, 1_000_000 * UNIT, 6)).unwrap();
    f.reject_unchanged(f.release_ix(FOUNDER, 1, 6), "ReservedQuotaExceeded");
    f.conserved();
}

#[test]
fn one_successor_pending_for_both_roles_preserves_other_role_mask_when_one_cancels() {
    let mut f = Fixture::new(false, false);
    f.at_cliff();
    let new = clone_key(&f.depositor);
    f.propose_withdrawal(FOUNDER, &new, true);
    f.propose_withdrawal(TREASURY, &new, true);
    assert_eq!(
        f.read::<WithdrawalKeyV5>(f.key_record(new.pubkey()))
            .pending_mask,
        3
    );
    f.run(f.cancel_withdrawal_ix(FOUNDER, 1, new.pubkey(), new.pubkey()))
        .unwrap();
    assert_eq!(
        f.read::<WithdrawalKeyV5>(f.key_record(new.pubkey()))
            .pending_mask,
        2
    );
    f.reject_unchanged(f.approve_ix(10, UNIT, f.source), "WithdrawalPaused");
    f.time(f.config.t0 + CLIFF + CHANGE_NOTICE);
    f.run(f.execute_withdrawal_ix(TREASURY, 1, false)).unwrap();
    let record: WithdrawalKeyV5 = f.read(f.key_record(new.pubkey()));
    assert_eq!((record.history_mask, record.pending_mask), (2, 0));
    assert_eq!(f.p().withdrawal[0].current, f.founder.pubkey());
}
