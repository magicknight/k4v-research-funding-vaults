use ::launch_vault_v5::{
    identity, period_at, period_start, AnnualRule, ChangeProposalV5, LaunchConfig, LaunchPolicyV5,
    LaunchVaultV5, TreasuryApprovalV5, WithdrawalKeyV5, WithdrawalProposalV5, CLIFF, ID, PERIOD,
};
use anchor_lang::{prelude::Pubkey, AccountSerialize, AnchorSerialize, Discriminator, Space};

#[test]
fn frozen_identity_vector_matches_compiled_serialization() {
    let v: serde_json::Value = serde_json::from_str(include_str!(
        "../../../spec/LAUNCH_V5_IDENTITY_VECTOR_v1.json"
    ))
    .unwrap();
    let key = |name: &str| {
        Pubkey::new_from_array(
            hex::decode(v[name].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
        )
    };
    let n = |name: &str| v["config"][name].as_str().unwrap().parse::<u64>().unwrap();
    let annual_rule = |index: usize| {
        let rule = &v["config"]["annual_rules"][index];
        let value = |key: &str| rule[key].as_str().unwrap().parse::<u64>().unwrap();
        AnnualRule {
            start_period: value("start_period"),
            end_period: value("end_period"),
            founder_basis: value("founder_basis"),
            treasury_basis: value("treasury_basis"),
            shared_cap: value("shared_cap"),
            release_bps: value("release_bps") as u16,
            source_hash: hex::decode(rule["source_hash"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
        }
    };
    let config = LaunchConfig {
        t0: n("t0") as i64,
        founder_amount: n("founder_amount"),
        treasury_amount: n("treasury_amount"),
        founder_period_cap: n("founder_period_cap"),
        treasury_period_cap: n("treasury_period_cap"),
        shared_hard_cap: n("shared_hard_cap"),
        max_report_age: n("max_report_age") as i64,
        annual_rules: [annual_rule(0), annual_rule(1)],
        recovery_keys: [0, 1, 2].map(|i| {
            Pubkey::new_from_array(
                hex::decode(v["config"]["recovery_keys"][i].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap(),
            )
        }),
        founder_recovery_keys: [0, 1, 2].map(|i| {
            Pubkey::new_from_array(
                hex::decode(v["config"]["founder_recovery_keys"][i].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap(),
            )
        }),
        treasury_recovery_keys: [0, 1, 2].map(|i| {
            Pubkey::new_from_array(
                hex::decode(v["config"]["treasury_recovery_keys"][i].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap(),
            )
        }),
    };
    assert_eq!(ID, key("program_hex"));
    let actual = identity(
        &key("creator_hex"),
        &key("mint_hex"),
        &key("founder_hex"),
        &key("treasury_hex"),
        &key("oracle_hex"),
        &key("specHash_hex").to_bytes(),
        &config,
    );
    assert_eq!(hex::encode(actual), v["identity_hex"]);
    let mut bytes = vec![];
    config.serialize(&mut bytes).unwrap();
    assert_eq!(hex::encode(bytes), v["config_borsh_hex"]);
}

#[test]
fn period_boundaries_and_extreme_timestamps_are_checked() {
    assert_eq!(CLIFF, 6 * PERIOD);
    assert!(period_at(10, 9).is_err());
    assert_eq!(period_at(10, 10 + PERIOD - 1).unwrap(), 0);
    assert_eq!(period_at(10, 10 + CLIFF).unwrap(), 6);
    assert!(period_at(i64::MIN, i64::MAX).is_err());
    assert!(period_start(i64::MAX, 1).is_err());
    assert!(period_start(0, u64::MAX).is_err());
    assert_eq!(period_start(-PERIOD, 1).unwrap(), 0);
}

#[test]
fn committed_idl_discriminators_and_account_sizes_match_compiled_abi() {
    // This test uses a generated interface, not a second handwritten field list.
    let idl: serde_json::Value =
        serde_json::from_str(include_str!("../../../idl/launch_vault_v5.json")).unwrap();
    assert_eq!(idl["address"], ID.to_string());
    let type_size = |name: &str| {
        fn size(ty: &serde_json::Value, all: &serde_json::Value) -> usize {
            match ty.as_str() {
                Some("u8" | "bool") => 1,
                Some("u16") => 2,
                Some("u64" | "i64") => 8,
                Some("pubkey") => 32,
                _ if ty.get("array").is_some() => {
                    size(&ty["array"][0], all) * ty["array"][1].as_u64().unwrap() as usize
                }
                _ => {
                    let name = ty["defined"]["name"].as_str().unwrap();
                    all.as_array()
                        .unwrap()
                        .iter()
                        .find(|t| t["name"] == name)
                        .unwrap()["type"]["fields"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|f| size(&f["type"], all))
                        .sum()
                }
            }
        }
        size(&serde_json::json!({"defined":{"name":name}}), &idl["types"])
    };
    for (name, discriminator, bytes) in [
        (
            "WithdrawalKeyV5",
            WithdrawalKeyV5::DISCRIMINATOR,
            WithdrawalKeyV5::INIT_SPACE,
        ),
        (
            "WithdrawalProposalV5",
            WithdrawalProposalV5::DISCRIMINATOR,
            WithdrawalProposalV5::INIT_SPACE,
        ),
        (
            "ChangeProposalV5",
            ChangeProposalV5::DISCRIMINATOR,
            ChangeProposalV5::INIT_SPACE,
        ),
        (
            "LaunchPolicyV5",
            LaunchPolicyV5::DISCRIMINATOR,
            LaunchPolicyV5::INIT_SPACE,
        ),
        (
            "LaunchVaultV5",
            LaunchVaultV5::DISCRIMINATOR,
            LaunchVaultV5::INIT_SPACE,
        ),
        (
            "TreasuryApprovalV5",
            TreasuryApprovalV5::DISCRIMINATOR,
            TreasuryApprovalV5::INIT_SPACE,
        ),
    ] {
        let a = idl["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap();
        let committed: Vec<u8> = serde_json::from_value(a["discriminator"].clone()).unwrap();
        assert_eq!(committed, discriminator);
        assert_eq!(type_size(name), bytes);
    }
    let vault = LaunchVaultV5 {
        policy: Pubkey::new_unique(),
        depositor: Pubkey::new_unique(),
        authority: Pubkey::new_unique(),
        role: 0,
        bump: 255,
        principal: u64::MAX,
        released_total: 0,
        period: 6,
        period_used: 0,
    };
    let mut data = vec![];
    vault.try_serialize(&mut data).unwrap();
    assert_eq!(data.len(), 8 + LaunchVaultV5::INIT_SPACE);
}
