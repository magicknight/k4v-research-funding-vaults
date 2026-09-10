// Appended to the frozen financial fixture by materialize_e11b_v7.py.
// All keys and account injections are local test fixtures, never production.

#[test]
fn e11b_preparation_has_no_custody_and_cannot_impersonate_policy() {
    let mut f = Fixture::new(false, false);
    let before = f.balance(f.source);
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    let bytes = f.svm.get_account(&f.preparation()).unwrap().data;
    assert!(f.svm.get_account(&f.policy).is_none());
    assert!(f.svm.get_account(&f.vault(FOUNDER)).is_none());
    assert!(f.run_many(vec![f.deposit_ix(FOUNDER, f.config.founder_amount)], f.creator.pubkey()).is_err());
    let fake = ix(accounts::PolicyOnly { policy: f.preparation() }, instruction::Activate {});
    assert!(f.run_many(vec![fake], f.creator.pubkey()).is_err());
    assert_eq!(f.balance(f.source), before);
    assert_eq!(f.svm.get_account(&f.preparation()).unwrap().data, bytes);
}

#[test]
fn e11b_each_of_six_consents_is_required_with_separate_fee_payer() {
    let mut f = Fixture::new(false, false);
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    let bytes = f.svm.get_account(&f.preparation()).unwrap().data;
    let actors = [f.creator.pubkey(), f.founder.pubkey(), f.treasury.pubkey(),
        f.recovery[0].pubkey(), f.recovery[1].pubkey(), f.recovery[2].pubkey()];
    for (i, actor) in actors.iter().enumerate() { assert!(!actors[..i].contains(actor)); }
    for index in [0, 1, 2, 4, 5, 6] {
        let mut unsigned = f.open_ix();
        unsigned.accounts[index].is_signer = false;
        assert!(f.run_many(vec![unsigned], f.outsider.pubkey()).is_err(), "missing role {index}");
        assert!(f.svm.get_account(&f.policy).is_none());
        let mut substituted = f.open_ix();
        substituted.accounts[index].pubkey = f.outsider.pubkey();
        assert!(f.run_many(vec![substituted], f.creator.pubkey()).is_err(), "substituted role {index}");
        assert!(f.svm.get_account(&f.policy).is_none());
        assert_eq!(f.svm.get_account(&f.preparation()).unwrap().data, bytes);
    }
    f.run_many(vec![f.open_ix()], f.outsider.pubkey()).unwrap();
    assert_eq!(f.p().creator, f.creator.pubkey());
    assert_eq!(f.p().withdrawal[0].current, f.founder.pubkey());
    assert_eq!(f.p().withdrawal[1].current, f.treasury.pubkey());
    assert_eq!(f.svm.get_account(&f.preparation()).unwrap().data, bytes);
}

#[test]
fn e11b_wrong_owner_discriminator_program_identity_and_mutated_config_are_rejected() {
    let mut f = Fixture::new(false, false);
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    let address = f.preparation();
    let original = f.svm.get_account(&address).unwrap();
    // Mutation injections test verifier defenses; no real program update path exists.
    for variant in 0..5 {
        let mut a = original.clone();
        match variant {
            0 => a.owner = solana_system_interface::program::ID,
            1 => a.data[0] ^= 1,
            2 => a.data[8 + 5 * 32] ^= 1, // bound program
            3 => a.data[8 + 6 * 32] ^= 1, // content identity / PDA
            _ => a.data[8 + 6 * 32 + 64] ^= 1, // config T0, keeping the old address
        }
        f.svm.set_account(address, a).unwrap();
        assert!(f.run_many(vec![f.open_ix()], f.creator.pubkey()).is_err(), "mutation {variant}");
        assert!(f.svm.get_account(&f.policy).is_none());
        f.svm.set_account(address, original.clone()).unwrap();
    }
    f.run_many(vec![f.open_ix()], f.creator.pubkey()).unwrap();
    assert_eq!(f.p().identity, f.hash);
    assert_eq!(f.svm.get_account(&address).unwrap().data, original.data);
}

#[test]
fn e11b_new_configuration_cannot_reuse_old_consent_or_policy_address() {
    let mut f = Fixture::new(false, false);
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    let old_policy = f.policy;
    let old_preparation = f.preparation();
    let mut old_consent = f.open_ix();
    f.config.t0 += 1;
    f.rebind();
    assert_ne!(f.preparation(), old_preparation);
    assert_ne!(f.policy, old_policy);
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    old_consent.accounts[8].pubkey = f.preparation();
    assert!(f.run_many(vec![old_consent], f.creator.pubkey()).is_err());
    assert!(f.svm.get_account(&old_policy).is_none());
    assert!(f.svm.get_account(&f.policy).is_none());
    let mut wrong_mint = f.open_ix();
    wrong_mint.accounts[7].pubkey = f.source;
    assert!(f.run_many(vec![wrong_mint], f.creator.pubkey()).is_err());
    f.run_many(vec![f.open_ix()], f.creator.pubkey()).unwrap();
    assert_eq!(f.p().config.t0, f.config.t0);
}

#[test]
fn e11b_expiry_replay_and_repreparation_never_reset_authority_or_time() {
    let mut expired = Fixture::new(false, false);
    expired.run_many(vec![expired.prepare_ix()], expired.creator.pubkey()).unwrap();
    expired.time(expired.config.t0);
    assert!(expired.run_many(vec![expired.open_ix()], expired.creator.pubkey()).is_err());
    assert!(expired.svm.get_account(&expired.policy).is_none());
    let mut f = Fixture::new(false, false);
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    let prepared = f.svm.get_account(&f.preparation()).unwrap().data;
    assert!(f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).is_err());
    f.run_many(vec![f.open_ix()], f.creator.pubkey()).unwrap();
    let policy = f.svm.get_account(&f.policy).unwrap().data;
    assert!(f.run_many(vec![f.open_ix()], f.creator.pubkey()).is_err());
    assert_eq!(f.svm.get_account(&f.policy).unwrap().data, policy);
    assert_eq!(f.svm.get_account(&f.preparation()).unwrap().data, prepared);
}

#[test]
fn e11b_mint_authorities_checked_at_prepare_and_again_at_consent() {
    let mut f = Fixture::new(false, false);
    let clean = f.svm.get_account(&f.mint).unwrap();
    for freeze in [false, true] {
        let mut a = clean.clone();
        let mut mint = Mint::unpack(&a.data).unwrap();
        if freeze { mint.freeze_authority = COption::Some(f.creator.pubkey()); }
        else { mint.mint_authority = COption::Some(f.creator.pubkey()); }
        Mint::pack(mint, &mut a.data).unwrap();
        f.svm.set_account(f.mint, a).unwrap();
        assert!(f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).is_err());
        assert!(f.svm.get_account(&f.preparation()).is_none());
        f.svm.set_account(f.mint, clean.clone()).unwrap();
    }
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    for freeze in [false, true] {
        let mut a = clean.clone();
        let mut mint = Mint::unpack(&a.data).unwrap();
        if freeze { mint.freeze_authority = COption::Some(f.creator.pubkey()); }
        else { mint.mint_authority = COption::Some(f.creator.pubkey()); }
        Mint::pack(mint, &mut a.data).unwrap();
        f.svm.set_account(f.mint, a).unwrap();
        assert!(f.run_many(vec![f.open_ix()], f.creator.pubkey()).is_err());
        assert!(f.svm.get_account(&f.policy).is_none());
        f.svm.set_account(f.mint, clean.clone()).unwrap();
    }
    f.run_many(vec![f.open_ix()], f.creator.pubkey()).unwrap();
}

#[test]
fn e11b_compiled_bootstrap_abi_has_compact_consent_and_fixed_preparation_size() {
    use anchor_lang::Space;
    let f = Fixture::new(false, false);
    assert_eq!(f.prepare_ix().data.len(), 564);
    assert_eq!(f.open_ix().data.len(), 8);
    assert_eq!(LaunchPreparationV7::INIT_SPACE, 6 * 32 + 64 + 492 + 1);
    assert_eq!(f.open_ix().accounts.iter().filter(|m| m.is_signer).count(), 6);
    assert!(!f.open_ix().accounts[8].is_writable);
    assert_eq!(f.open_ix().accounts[8].pubkey, f.preparation());
    assert_ne!(LaunchPreparationV7::DISCRIMINATOR, LaunchPolicyV7::DISCRIMINATOR);
}
