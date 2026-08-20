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

/// Everything a release needs to know about the shared window.
pub struct WindowInputs {
    pub now: i64,
    pub updated_at: i64,
    /// The sentinel for "this oracle has never spoken". A timestamp cannot
    /// carry that meaning: `updated_at == 0` is also a real instant, and a
    /// report made at it would be indistinguishable from no report at all.
    pub report_count: u64,
    pub max_age_seconds: i64,
    pub eligible_volume: u64,
    pub market_capacity_bps: u16,
    pub hard_ceiling: u64,
    pub silence_floor: u64,
    pub silence_grace_seconds: i64,
}

/// The window a release must fit into, or an error if there is none.
///
/// A fresh input gives the ordinary window. A stale one refuses, exactly as
/// before, unless the policy declared a silence floor at creation and the
/// silence has lasted long enough to engage it. The floor is deliberately not a
/// fallback to the last reported volume: it is a fixed number chosen in advance
/// and small enough that going quiet is never better than reporting honestly.
pub fn window_capacity(inputs: WindowInputs) -> Result<u64> {
    // A policy whose oracle has never spoken releases nothing, whatever else is
    // declared. No floor and no elapsed time can substitute for a first report.
    require!(inputs.report_count > 0, CovenantError::StaleMarketInput);

    if assert_fresh(inputs.now, inputs.updated_at, inputs.max_age_seconds).is_ok() {
        return effective_capacity(
            inputs.eligible_volume,
            inputs.market_capacity_bps,
            inputs.hard_ceiling,
        );
    }

    // The stale branch. A policy that declared no floor fails closed here, which
    // is the whole behaviour before the floor existed.
    require!(inputs.silence_floor > 0, CovenantError::StaleMarketInput);
    let age = inputs
        .now
        .checked_sub(inputs.updated_at)
        .ok_or(CovenantError::InvalidClock)?;
    require!(
        age >= inputs.silence_grace_seconds,
        CovenantError::StaleMarketInput
    );
    Ok(inputs.silence_floor.min(inputs.hard_ceiling))
}

/// A stale input rejects rather than reusing the last value, and an input dated
/// in the future is not treated as maximally fresh. Whether the oracle has ever
/// spoken is a separate question, answered by `report_count`, not by this.
pub fn assert_fresh(now: i64, updated_at: i64, max_age_seconds: i64) -> Result<()> {
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
    use crate::constants::{
        APPROVAL_SEED, MARKET_SEED, MIN_SILENCE_GRACE_SECONDS, POLICY_SEED, TOKEN_VAULT_SEED,
        VAULT_SEED,
    };
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

    fn inputs(now: i64, updated_at: i64, floor: u64, grace: i64) -> WindowInputs {
        WindowInputs {
            now,
            updated_at,
            report_count: 1,
            max_age_seconds: 3 * 24 * 60 * 60,
            eligible_volume: 120_000_000,
            market_capacity_bps: 250,
            hard_ceiling: u64::MAX,
            silence_floor: floor,
            silence_grace_seconds: grace,
        }
    }

    const GRACE: i64 = MIN_SILENCE_GRACE_SECONDS;
    const REPORTED_AT: i64 = 1_000_000;

    #[test]
    fn a_policy_whose_oracle_never_spoke_has_no_window_at_all() {
        let mut w = inputs(REPORTED_AT, REPORTED_AT, 0, 0);
        w.report_count = 0;
        assert!(window_capacity(w).is_err());
    }

    #[test]
    fn without_a_declared_floor_a_stale_input_still_refuses_forever() {
        let fresh = REPORTED_AT + 24 * 60 * 60;
        assert_eq!(
            window_capacity(inputs(fresh, REPORTED_AT, 0, 0)).unwrap(),
            3_000_000
        );
        for elapsed in [4 * 24 * 60 * 60, GRACE, 100 * GRACE] {
            assert!(window_capacity(inputs(REPORTED_AT + elapsed, REPORTED_AT, 0, 0)).is_err());
        }
    }

    #[test]
    fn a_declared_floor_engages_only_after_the_grace_period() {
        let floor = 30_000;
        // Fresh: the ordinary window, not the floor.
        let fresh = REPORTED_AT + 24 * 60 * 60;
        assert_eq!(
            window_capacity(inputs(fresh, REPORTED_AT, floor, GRACE)).unwrap(),
            3_000_000
        );
        // Stale but inside the grace period: still refused. This is the gap the
        // oracle rotation is meant to be repaired in.
        assert!(window_capacity(inputs(
            REPORTED_AT + 4 * 24 * 60 * 60,
            REPORTED_AT,
            floor,
            GRACE
        ))
        .is_err());
        assert!(
            window_capacity(inputs(REPORTED_AT + GRACE - 1, REPORTED_AT, floor, GRACE)).is_err()
        );
        // At the grace boundary and beyond: the floor, and only the floor.
        assert_eq!(
            window_capacity(inputs(REPORTED_AT + GRACE, REPORTED_AT, floor, GRACE)).unwrap(),
            floor
        );
        assert_eq!(
            window_capacity(inputs(REPORTED_AT + 10 * GRACE, REPORTED_AT, floor, GRACE)).unwrap(),
            floor
        );
    }

    #[test]
    fn a_floor_cannot_resurrect_a_policy_whose_oracle_never_spoke() {
        // No amount of elapsed time turns "never reported" into a measurable
        // silence. Otherwise a policy could be opened, left alone, and start
        // releasing on its own.
        for now in [GRACE, 10 * GRACE, i64::MAX / 2] {
            let mut w = inputs(now, 0, 30_000, GRACE);
            w.report_count = 0;
            assert!(window_capacity(w).is_err());
        }
        // And the same instant, once the oracle has actually spoken at it, is a
        // real report with a real age. The count is the sentinel; the timestamp
        // is a timestamp.
        assert_eq!(
            window_capacity(inputs(GRACE, 0, 30_000, GRACE)).unwrap(),
            30_000
        );
    }

    #[test]
    fn the_hard_ceiling_also_bounds_the_silence_floor() {
        let mut w = inputs(REPORTED_AT + GRACE, REPORTED_AT, 30_000, GRACE);
        w.hard_ceiling = 10_000;
        assert_eq!(window_capacity(w).unwrap(), 10_000);
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
    fn freshness_rejects_expired_and_future_inputs() {
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
