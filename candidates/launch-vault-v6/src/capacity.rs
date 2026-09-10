//! Pure integer projection. Current-period releases are excluded from the
//! prior-period budgets, so repeated calls cannot change quota weights.
use crate::state::*;
use anchor_lang::prelude::*;

pub fn annual_cap(basis: u64, bps: u16) -> Result<u64> {
    require!(bps <= MAX_RELEASE_BPS, LaunchError::InvalidAnnualRule);
    Ok((u128::from(basis) * u128::from(bps) / 10_000) as u64)
}

pub fn reserved_quotas(capacity: u64, founder: u64, treasury: u64, eligible: bool) -> (u64, u64) {
    if !eligible {
        return (0, capacity.min(treasury));
    }
    let denominator = u128::from(founder) + u128::from(treasury);
    if denominator == 0 {
        return (0, 0);
    }
    let available = u128::from(capacity).min(denominator);
    // available and founder are each <= u64::MAX, hence the product fits u128.
    let f = available * u128::from(founder) / denominator;
    (f as u64, (available - f) as u64)
}

pub fn validate_rules(config: &LaunchConfig) -> Result<()> {
    require!(
        config.annual_rules[0].start_period == 0,
        LaunchError::InvalidAnnualRule
    );
    require!(
        config.annual_rules[0].end_period == config.annual_rules[1].start_period,
        LaunchError::InvalidAnnualRule
    );
    for rule in config.annual_rules {
        require!(
            rule.end_period > rule.start_period
                && rule.founder_basis <= config.founder_amount
                && rule.treasury_basis <= config.treasury_amount
                && rule.source_hash != [0; 32],
            LaunchError::InvalidAnnualRule
        );
        let f = annual_cap(rule.founder_basis, rule.release_bps)?;
        let t = annual_cap(rule.treasury_basis, rule.release_bps)?;
        require!(
            u128::from(rule.shared_cap) <= u128::from(f) + u128::from(t),
            LaunchError::InvalidAnnualRule
        );
        period_start(config.t0, rule.end_period)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowLimits {
    pub period: u64,
    pub annual_index: u8,
    pub period_used: [u64; 2],
    pub annual_used: [u64; 2],
    pub annual_caps: [u64; 2],
    pub caps: [u64; 2],
    pub quotas: [u64; 2],
    pub capacity: u64,
    pub correction_pause: bool,
}

fn sub(a: u64, b: u64) -> Result<u64> {
    a.checked_sub(b)
        .ok_or_else(|| error!(LaunchError::InvalidAccounting))
}

pub fn window_limits(p: &LaunchPolicyV6, now: i64) -> Result<WindowLimits> {
    let period = period_at(p.config.t0, now)?;
    require!(period >= p.period, LaunchError::ClockRollback);
    let index = p
        .config
        .annual_rules
        .iter()
        .position(|r| period >= r.start_period && period < r.end_period)
        .ok_or(LaunchError::InvalidAnnualRule)?;
    require!(
        index >= usize::from(p.annual_index),
        LaunchError::ClockRollback
    );
    require!(
        p.shared_used
            == p.founder_period_used
                .checked_add(p.treasury_period_used)
                .ok_or(LaunchError::InvalidAccounting)?,
        LaunchError::InvalidAccounting
    );
    let used = if period == p.period {
        [p.founder_period_used, p.treasury_period_used]
    } else {
        [0, 0]
    };
    let annual_used = if index == usize::from(p.annual_index) {
        [p.founder_annual_used, p.treasury_annual_used]
    } else {
        [0, 0]
    };
    let rule = p.config.annual_rules[index];
    let annual_caps = [
        annual_cap(rule.founder_basis, rule.release_bps)?,
        annual_cap(rule.treasury_basis, rule.release_bps)?,
    ];
    let prior_annual = [sub(annual_used[0], used[0])?, sub(annual_used[1], used[1])?];
    let prior_lifetime = [
        sub(p.founder_released_total, used[0])?,
        sub(p.treasury_released_total, used[1])?,
    ];
    let principals = [p.config.founder_amount, p.config.treasury_amount];
    let configured_caps = [p.config.founder_period_cap, p.config.treasury_period_cap];
    let mut caps = [0, 0];
    for i in 0..2 {
        caps[i] = configured_caps[i]
            .min(annual_caps[i] / RATE_DIVISOR)
            .min(sub(annual_caps[i], prior_annual[i])?)
            .min(sub(principals[i], prior_lifetime[i])?);
    }
    let shared_prior = prior_annual[0]
        .checked_add(prior_annual[1])
        .ok_or(LaunchError::Overflow)?;
    let capacity = p
        .report_capacity
        .min(p.config.shared_hard_cap)
        .min(sub(rule.shared_cap, shared_prior)?);
    let (f, t) = reserved_quotas(
        capacity,
        caps[0],
        caps[1],
        now >= p
            .config
            .t0
            .checked_add(CLIFF)
            .ok_or(LaunchError::Overflow)?,
    );
    let quotas = [f, t];
    Ok(WindowLimits {
        period,
        annual_index: index as u8,
        period_used: used,
        annual_used,
        annual_caps,
        caps,
        quotas,
        capacity,
        correction_pause: used[0] > f || used[1] > t,
    })
}

pub fn consume(p: &mut LaunchPolicyV6, v: &mut LaunchVaultV6, now: i64, amount: u64) -> Result<()> {
    let limits = window_limits(p, now)?;
    require!(
        !limits.correction_pause,
        LaunchError::CapacityCorrectionPause
    );
    let role = usize::from(v.role);
    require!(role < 2, LaunchError::InvalidConfig);
    let total = [p.founder_released_total, p.treasury_released_total];
    require!(
        v.released_total == total[role],
        LaunchError::InvalidAccounting
    );
    let mut used = limits.period_used;
    let mut annual_used = limits.annual_used;
    used[role] = used[role]
        .checked_add(amount)
        .ok_or(LaunchError::Overflow)?;
    annual_used[role] = annual_used[role]
        .checked_add(amount)
        .ok_or(LaunchError::Overflow)?;
    let lifetime = total[role]
        .checked_add(amount)
        .ok_or(LaunchError::Overflow)?;
    let shared = used[0].checked_add(used[1]).ok_or(LaunchError::Overflow)?;
    require!(
        used[role] <= limits.quotas[role],
        LaunchError::ReservedQuotaExceeded
    );
    require!(
        annual_used[role] <= limits.annual_caps[role]
            && lifetime <= v.principal
            && shared <= limits.capacity,
        LaunchError::CapacityExceeded
    );
    // No state is committed before all arithmetic and accounting checks pass.
    p.period = limits.period;
    p.annual_index = limits.annual_index;
    p.founder_period_used = used[0];
    p.treasury_period_used = used[1];
    p.founder_annual_used = annual_used[0];
    p.treasury_annual_used = annual_used[1];
    p.shared_used = shared;
    if role == 0 {
        p.founder_released_total = lifetime;
    } else {
        p.treasury_released_total = lifetime;
    }
    v.period = limits.period;
    v.period_used = used[role];
    v.released_total = lifetime;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_integer_quotas_round_only_one_unit_toward_treasury() {
        assert_eq!(
            reserved_quotas(u64::MAX, u64::MAX, u64::MAX, true),
            (u64::MAX / 2, u64::MAX / 2 + 1)
        );
        assert_eq!(reserved_quotas(u64::MAX, u64::MAX, 0, true), (u64::MAX, 0));
        assert_eq!(reserved_quotas(5, 0, 0, true), (0, 0));
        assert_eq!(reserved_quotas(5, 99, 3, false), (0, 3));
        assert_eq!(reserved_quotas(5, 3, 7, true), (1, 4));
        assert_eq!(annual_cap(u64::MAX, 500).unwrap(), 922_337_203_685_477_580);
        assert!(annual_cap(100, 501).is_err());
    }
}
