//! TEST_ONLY immutable preparation admission; no transfers or mutable config API.
use crate::{capacity, governance, state::*};
use anchor_lang::prelude::*;
use anchor_spl::token::Mint;

pub fn validate(p: &LaunchPreparationV7, mint: &Mint, now: i64) -> Result<()> {
    require!(
        cfg!(feature = "test-profile"),
        LaunchError::ExperimentalProfileDisabled
    );
    let config = &p.config;
    require!(config.t0 > now, LaunchError::T0Boundary);
    config.t0.checked_add(CLIFF).ok_or(LaunchError::Overflow)?;
    capacity::validate_rules(config)?;
    governance::validate_recovery_keys(&config.recovery_keys)?;
    for (keys, actor) in [
        (&config.founder_recovery_keys, p.founder),
        (&config.treasury_recovery_keys, p.treasury),
    ] {
        governance::validate_recovery_keys(keys)?;
        require!(!keys.contains(&actor), LaunchError::InvalidConfig);
    }
    require!(
        config
            .founder_amount
            .checked_add(config.treasury_amount)
            .ok_or(LaunchError::Overflow)?
            <= mint.supply,
        LaunchError::InvalidConfig
    );
    require!(
        config.founder_amount > 0
            && config.treasury_amount > 0
            && config.founder_period_cap > 0
            && config.founder_period_cap <= config.founder_amount
            && config.treasury_period_cap > 0
            && config.treasury_period_cap <= config.treasury_amount
            && config.shared_hard_cap > 0
            && (1..=7 * 86_400).contains(&config.max_report_age)
            && p.spec_hash != [0; 32]
            && p.oracle != Pubkey::default(),
        LaunchError::InvalidConfig
    );
    require!(
        mint.mint_authority.is_none() && mint.freeze_authority.is_none(),
        LaunchError::MintAuthorityLive
    );
    require!(
        identity(
            &p.creator,
            &p.mint,
            &p.founder,
            &p.treasury,
            &p.oracle,
            &p.spec_hash,
            config
        ) == p.identity,
        LaunchError::IdentityMismatch
    );
    Ok(())
}
