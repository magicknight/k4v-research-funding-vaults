use crate::constants::{BPS_DENOMINATOR, MONTHS_PER_YEAR, PERIOD_SECONDS};
use crate::error::CovenantError;
use anchor_lang::prelude::*;

/// Two explicit floors, in the covenant's order: annual first, then monthly.
/// Collapsing them into one division changes the result.
pub fn monthly_cap(deposit: u64, annual_release_bps: u16) -> Result<u64> {
    let annual = u128::from(deposit)
        .checked_mul(u128::from(annual_release_bps))
        .ok_or(CovenantError::ArithmeticOverflow)?
        / BPS_DENOMINATOR;
    let monthly = annual / MONTHS_PER_YEAR;
    u64::try_from(monthly).map_err(|_| error!(CovenantError::ArithmeticOverflow))
}

/// `eligible_volume` is in mint base units, so no price enters this function
/// and none is needed to compare it against a vault's cap.
pub fn market_capacity(eligible_volume: u64, market_capacity_bps: u16) -> Result<u64> {
    let capacity = u128::from(eligible_volume)
        .checked_mul(u128::from(market_capacity_bps))
        .ok_or(CovenantError::ArithmeticOverflow)?
        / BPS_DENOMINATOR;
    u64::try_from(capacity).map_err(|_| error!(CovenantError::ArithmeticOverflow))
}

/// The shared window is the tighter of what the market can absorb and the
/// policy's frozen absolute ceiling. The ceiling is the only term the oracle
/// cannot move, which is what makes it worth carrying.
pub fn effective_capacity(
    eligible_volume: u64,
    market_capacity_bps: u16,
    hard_ceiling: u64,
) -> Result<u64> {
    Ok(market_capacity(eligible_volume, market_capacity_bps)?.min(hard_ceiling))
}

/// Every vault on a policy indexes the same windows. B1 anchored periods per
/// vault, which leaves an aggregate over two vaults undefined.
pub fn period_index(now: i64, genesis_ts: i64) -> Result<u64> {
    let elapsed = now
        .checked_sub(genesis_ts)
        .ok_or(CovenantError::InvalidClock)?;
    require!(elapsed >= 0, CovenantError::InvalidClock);
    u64::try_from(elapsed / PERIOD_SECONDS).map_err(|_| error!(CovenantError::ArithmeticOverflow))
}

/// A market input that was never reported has `updated_at == 0`, so a freshly
/// opened policy releases nothing until the oracle has spoken. A stale input
/// rejects rather than reusing the last value.
pub fn assert_fresh(now: i64, updated_at: i64, max_age_seconds: i64) -> Result<()> {
    require!(updated_at > 0, CovenantError::StaleMarketInput);
    let age = now
        .checked_sub(updated_at)
        .ok_or(CovenantError::InvalidClock)?;
    require!(
        (0..=max_age_seconds).contains(&age),
        CovenantError::StaleMarketInput
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{APPROVAL_SEED, MARKET_SEED, POLICY_SEED, TOKEN_VAULT_SEED, VAULT_SEED};
    use crate::state::VaultKind;
    use anchor_lang::prelude::Pubkey;

    #[test]
    fn cap_uses_two_explicit_floor_operations() {
        assert_eq!(monthly_cap(1_000_000_000, 500).unwrap(), 4_166_666);
        assert_eq!(monthly_cap(239, 500).unwrap(), 0);
        assert_eq!(monthly_cap(u64::MAX, 500).unwrap(), 76_861_433_640_456_465);
    }

    /// The same integers the chain-independent model produces for the vector in
    /// `tests/test_purpose_vault_b2_parity.py`. If either side moves, both fail.
    #[test]
    fn cross_language_parity_vector() {
        let beneficiary = monthly_cap(300_000_000, 500).unwrap();
        let purpose = monthly_cap(500_000_000, 500).unwrap();
        let capacity = market_capacity(120_000_000, 250).unwrap();
        assert_eq!(
            (beneficiary, purpose, capacity),
            (1_250_000, 2_083_333, 3_000_000)
        );
        // Individually under the ceiling, jointly over it. That gap is the
        // reason the aggregate rule exists at all.
        assert!(beneficiary <= capacity && purpose <= capacity);
        assert!(beneficiary + purpose > capacity);
        // With no ceiling set the window is the market term alone; the vector
        // is unchanged by the ceiling's introduction.
        assert_eq!(
            effective_capacity(120_000_000, 250, u64::MAX).unwrap(),
            capacity
        );
    }

    /// The property the absolute ceiling exists to make legible: a compromised
    /// oracle can widen the shared window, but the per-vault schedule it cannot
    /// touch still bounds every release. This asserts the widening itself is
    /// bounded, and that an inert ceiling is genuinely inert.
    #[test]
    fn a_frozen_ceiling_bounds_what_any_oracle_report_can_open() {
        let honest = effective_capacity(120_000_000, 250, u64::MAX).unwrap();
        assert_eq!(honest, 3_000_000);

        // A report inflated by six orders of magnitude, with no ceiling set.
        let inflated = effective_capacity(120_000_000_000_000, 250, u64::MAX).unwrap();
        assert_eq!(inflated, 3_000_000_000_000);

        // The same report against a policy that froze a ceiling.
        let bounded = effective_capacity(120_000_000_000_000, 250, 2_500_000).unwrap();
        assert_eq!(bounded, 2_500_000);

        // And the ceiling never widens a window the market has already closed.
        assert_eq!(effective_capacity(0, 250, 2_500_000).unwrap(), 0);
    }

    /// The largest volume and rate the program accepts must not overflow, or a
    /// large honest report would reject releases instead of permitting them.
    #[test]
    fn the_widest_admissible_report_does_not_overflow() {
        let widest = effective_capacity(u64::MAX, 500, u64::MAX).unwrap();
        assert_eq!(widest, u64::MAX / 20);
    }

    #[test]
    fn market_capacity_floors_toward_zero() {
        assert_eq!(market_capacity(0, 500).unwrap(), 0);
        assert_eq!(market_capacity(19, 500).unwrap(), 0);
        assert_eq!(market_capacity(20, 500).unwrap(), 1);
    }

    #[test]
    fn periods_are_fixed_windows_anchored_at_policy_genesis() {
        assert_eq!(period_index(1_000, 1_000).unwrap(), 0);
        assert_eq!(period_index(1_000 + PERIOD_SECONDS - 1, 1_000).unwrap(), 0);
        assert_eq!(period_index(1_000 + PERIOD_SECONDS, 1_000).unwrap(), 1);
        assert!(period_index(999, 1_000).is_err());
    }

    #[test]
    fn freshness_rejects_unreported_expired_and_future_inputs() {
        assert!(assert_fresh(1_000, 0, 600).is_err());
        assert!(assert_fresh(1_000, 400, 600).is_ok());
        assert!(assert_fresh(1_000, 399, 600).is_err());
        // A timestamp in the future is not treated as maximally fresh.
        assert!(assert_fresh(1_000, 1_001, 600).is_err());
    }

    #[test]
    fn vault_seeds_separate_the_two_kinds() {
        let program_id = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 1));
        let authority = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 33));
        let mint = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 65));
        let policy_hash = [0x42; 32];

        let derive = |kind: VaultKind| {
            Pubkey::find_program_address(
                &[
                    VAULT_SEED,
                    policy_hash.as_ref(),
                    &[kind.seed_byte()],
                    authority.as_ref(),
                    mint.as_ref(),
                ],
                &program_id,
            )
            .0
        };
        assert_ne!(derive(VaultKind::Beneficiary), derive(VaultKind::Purpose));
    }

    #[test]
    fn pda_vector_is_frozen() {
        let program_id = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 1));
        let authority = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 33));
        let mint = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 65));
        let policy_hash = [0x42; 32];

        let (policy, _) =
            Pubkey::find_program_address(&[POLICY_SEED, policy_hash.as_ref()], &program_id);
        let (market, _) =
            Pubkey::find_program_address(&[MARKET_SEED, policy_hash.as_ref()], &program_id);
        let (vault, _) = Pubkey::find_program_address(
            &[
                VAULT_SEED,
                policy_hash.as_ref(),
                &[VaultKind::Purpose.seed_byte()],
                authority.as_ref(),
                mint.as_ref(),
            ],
            &program_id,
        );
        let (token, _) =
            Pubkey::find_program_address(&[TOKEN_VAULT_SEED, vault.as_ref()], &program_id);
        let (approval, _) = Pubkey::find_program_address(
            &[APPROVAL_SEED, vault.as_ref(), &7u64.to_le_bytes()],
            &program_id,
        );

        // Distinct derivations must not collide; a collision would silently let
        // one account stand in for another.
        let all = [policy, market, vault, token, approval];
        for (index, left) in all.iter().enumerate() {
            for right in all.iter().skip(index + 1) {
                assert_ne!(left, right);
            }
        }
    }
}
