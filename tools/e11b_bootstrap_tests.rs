//! Extra signed-runtime bootstrap tests. set_account is adversarial fault injection.
use super::*;

fn prepared() -> Fixture {
    let mut f = Fixture::new(false, false);
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    assert_eq!(f.last_signers.len(), 1);
    assert!(f.svm.get_account(&f.policy).is_none());
    assert_eq!(f.svm.get_account(&f.preparation()).unwrap().data.len(), 725);
    f
}

#[test]
fn compact_confirmation_has_six_distinct_signatures_and_no_configuration_payload() {
    let mut f = prepared();
    let before = f.svm.get_account(&f.preparation()).unwrap().data;
    let open = f.open_ix();
    assert_eq!(open.data.len(), 8);
    assert!(!open.accounts[8].is_writable);
    f.run_many(vec![open], f.creator.pubkey()).unwrap();
    assert_eq!(f.last_signers.len(), 6);
    assert_eq!(f.p().identity, f.hash);
    assert_eq!(f.p().state, PREPARED);
    assert_eq!(f.p().funded_mask, 0);
    assert_eq!(f.p().config.t0, f.config.t0);
    assert_eq!(f.svm.get_account(&f.preparation()).unwrap().data, before);
}

#[test]
fn every_designated_signature_is_required_including_creator_with_external_payer() {
    for index in [0, 1, 2, 4, 5, 6] {
        let mut f = prepared();
        let mut open = f.open_ix();
        open.accounts[index].is_signer = false;
        let before = f.svm.get_account(&f.preparation()).unwrap().data;
        let result = f.run_many(vec![open], f.outsider.pubkey());
        rejected(result, "AccountNotSigner");
        assert!(f.svm.get_account(&f.policy).is_none());
        assert_eq!(f.svm.get_account(&f.preparation()).unwrap().data, before);
    }
}

#[test]
fn fully_resigned_wrong_role_keys_cannot_adopt_a_preparation() {
    for index in [0, 1, 2, 3, 4, 5, 6] {
        let mut f = prepared();
        let mut open = f.open_ix();
        open.accounts[index].pubkey = f.outsider.pubkey();
        f.reject_unchanged(open, if index >= 4 { "Unauthorized" } else { "IdentityMismatch" });
        assert!(f.svm.get_account(&f.policy).is_none());
    }
}

#[test]
fn preparation_is_not_a_policy_and_cannot_be_closed_updated_or_recreated() {
    let mut f = prepared();
    assert!(f.run_many(vec![f.deposit_ix(FOUNDER, f.config.founder_amount)], f.creator.pubkey()).is_err());
    f.reject_unchanged(f.prepare_ix(), "already in use");
    for name in ["open_policy", "update_preparation", "close_preparation"] {
        let mut attack = f.prepare_ix();
        attack.data = solana_sha256_hasher::hash(format!("global:{name}").as_bytes()).to_bytes()[..8].to_vec();
        assert!(f.run_many(vec![attack], f.creator.pubkey()).is_err());
    }
    f.run(f.open_ix()).unwrap();
    f.run(f.cancel_ix()).unwrap();
    f.reject_unchanged(f.open_ix(), "already in use");
    f.reject_unchanged(f.prepare_ix(), "already in use");
}

#[test]
fn forged_owner_discriminator_configuration_and_seed_are_rejected() {
    for attack in ["owner", "discriminator", "config", "bump", "address"] {
        let mut f = prepared();
        let mut account = f.svm.get_account(&f.preparation()).unwrap();
        let mut open = f.open_ix();
        let mut address = f.preparation();
        match attack {
            "owner" => account.owner = f.outsider.pubkey(),
            "discriminator" => account.data[0] ^= 1,
            "config" => account.data[232] ^= 1,
            "bump" => *account.data.last_mut().unwrap() ^= 1,
            "address" => { address = Pubkey::new_unique(); open.accounts[8].pubkey = address; },
            _ => unreachable!(),
        }
        f.svm.set_account(address, account).unwrap();
        assert!(f.run_many(vec![open], f.creator.pubkey()).is_err(), "{attack}");
        assert!(f.svm.get_account(&f.policy).is_none(), "{attack}");
    }
}

#[test]
fn live_mint_authorities_and_past_t0_are_rechecked_at_both_stages() {
    for after_prepare in [false, true] {
        for attack in ["mint", "freeze", "supply", "time"] {
            let mut f = if after_prepare { prepared() } else { Fixture::new(false, false) };
            let mut a = f.svm.get_account(&f.mint).unwrap();
            let mut mint = Mint::unpack(&a.data).unwrap();
            match attack {
                "mint" => mint.mint_authority = COption::Some(f.creator.pubkey()),
                "freeze" => mint.freeze_authority = COption::Some(f.creator.pubkey()),
                "supply" => mint.supply = 1,
                "time" => f.time(f.config.t0),
                _ => unreachable!(),
            }
            Mint::pack(mint, &mut a.data).unwrap();
            f.svm.set_account(f.mint, a).unwrap();
            let action = if after_prepare { f.open_ix() } else { f.prepare_ix() };
            assert!(f.run_many(vec![action], f.creator.pubkey()).is_err(), "{attack}");
            assert!(f.svm.get_account(&f.policy).is_none());
        }
    }
}

#[test]
fn default_sbf_also_refuses_confirmation_of_a_valid_preparation() {
    let mut f = prepared();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/v7-disabled/launch_vault_v7.so");
    // Test-only program-cache replacement supplies otherwise valid prepared state.
    f.svm.add_program_from_file(ID, path).unwrap();
    f.reject_unchanged(f.open_ix(), "ExperimentalProfileDisabled");
    assert!(f.svm.get_account(&f.policy).is_none());
}

#[test]
fn signed_message_and_each_signature_bind_preparation_program_and_policy() {
    let mut f = prepared();
    let open = f.open_ix();
    f.svm.expire_blockhash();
    let signers = [&f.creator, &f.founder, &f.treasury, &f.recovery[0], &f.recovery[1], &f.recovery[2]];
    let tx = Transaction::new_signed_with_payer(&[open], Some(&f.creator.pubkey()), &signers, f.svm.latest_blockhash());
    assert_eq!(tx.signatures.len(), 6);
    for index in 0..6 {
        let mut missing = tx.clone();
        missing.signatures[index] = Default::default();
        assert!(format!("{:?}", f.svm.send_transaction(missing).unwrap_err()).contains("SignatureFailure"));
    }
    for index in [8, 9] {
        let mut changed = tx.clone();
        let key_index = changed.message.instructions[0].accounts[index] as usize;
        changed.message.account_keys[key_index] = Pubkey::new_unique();
        assert!(format!("{:?}", f.svm.send_transaction(changed).unwrap_err()).contains("SignatureFailure"));
    }
    let mut changed = tx.clone();
    let key_index = changed.message.instructions[0].program_id_index as usize;
    changed.message.account_keys[key_index] = Pubkey::new_unique();
    assert!(format!("{:?}", f.svm.send_transaction(changed).unwrap_err()).contains("SignatureFailure"));
    f.svm.send_transaction(tx).unwrap();
    assert!(f.run_many(vec![f.open_ix()], f.creator.pubkey()).is_err());
}

#[test]
fn outsider_prefunding_cannot_occupy_content_addressed_preparation_or_policy() {
    let mut f = Fixture::new(false, false);
    for destination in [f.preparation(), f.policy] {
        let donation = solana_system_interface::instruction::transfer(&f.outsider.pubkey(), &destination, 1_000_000);
        f.run_many(vec![donation], f.outsider.pubkey()).unwrap();
    }
    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();
    f.run_many(vec![f.open_ix()], f.creator.pubkey()).unwrap();
    assert_eq!(f.p().identity, f.hash);
    assert_eq!(f.last_signers.len(), 6);
}
