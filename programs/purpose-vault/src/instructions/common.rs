use crate::error::CovenantError;
use crate::policy;
use crate::state::{CovenantVault, MarketInput, PolicyWindow};
use anchor_lang::prelude::*;

/// The three counters every release must satisfy, in one place so the two
/// release paths cannot drift apart. Both kinds debit the same shared window;
/// that is the whole point of B2 over B1.
pub struct ReleaseGate<'a> {
    pub policy: &'a mut PolicyWindow,
    pub vault: &'a mut CovenantVault,
    pub market: &'a MarketInput,
}

pub fn apply_release(gate: ReleaseGate, now: i64, period: u64, amount: u64) -> Result<u64> {
    require!(amount > 0, CovenantError::ZeroAmount);

    // Resolved first and deliberately: without a window we do not know the
    // ceiling, and not knowing means no. A policy that declared no silence
    // floor refuses outright here, exactly as it always did.
    let capacity = policy::window_capacity(policy::WindowInputs {
        now,
        updated_at: gate.market.updated_at,
        report_count: gate.market.report_count,
        max_age_seconds: gate.market.max_age_seconds,
        eligible_volume: gate.market.eligible_volume,
        market_capacity_bps: gate.market.market_capacity_bps,
        hard_ceiling: gate.policy.hard_ceiling,
        silence_floor: gate.policy.silence_floor,
        silence_grace_seconds: gate.policy.silence_grace_seconds,
    })?;

    if period > gate.policy.current_period_index {
        gate.policy.current_period_index = period;
        gate.policy.released_this_period = 0;
    }
    if period > gate.vault.current_period_index {
        gate.vault.current_period_index = period;
        gate.vault.released_this_period = 0;
    }

    let next_vault_period = gate
        .vault
        .released_this_period
        .checked_add(amount)
        .ok_or(CovenantError::ArithmeticOverflow)?;
    require!(
        next_vault_period <= gate.vault.monthly_cap,
        CovenantError::PeriodCapExceeded
    );

    let next_total = gate
        .vault
        .released_total
        .checked_add(amount)
        .ok_or(CovenantError::ArithmeticOverflow)?;
    require!(
        next_total <= gate.vault.deposited_amount,
        CovenantError::DepositExceeded
    );

    let next_window = gate
        .policy
        .released_this_period
        .checked_add(amount)
        .ok_or(CovenantError::ArithmeticOverflow)?;
    require!(
        next_window <= capacity,
        CovenantError::AggregateCapacityExceeded
    );

    gate.vault.released_this_period = next_vault_period;
    gate.vault.released_total = next_total;
    gate.policy.released_this_period = next_window;
    Ok(capacity)
}
