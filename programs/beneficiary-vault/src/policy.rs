use crate::constants::{BPS_DENOMINATOR, MONTHS_PER_YEAR, PERIOD_SECONDS};
use crate::error::VaultError;
use anchor_lang::prelude::*;

pub fn monthly_cap(deposit: u64, annual_release_bps: u16) -> Result<u64> {
    let annual = u128::from(deposit)
        .checked_mul(u128::from(annual_release_bps))
        .ok_or(VaultError::ArithmeticOverflow)?
        / BPS_DENOMINATOR;
    let monthly = annual / MONTHS_PER_YEAR;
    u64::try_from(monthly).map_err(|_| error!(VaultError::ArithmeticOverflow))
}

pub fn period_index(now: i64, cliff_end_ts: i64) -> Result<u64> {
    let elapsed = now
        .checked_sub(cliff_end_ts)
        .ok_or(VaultError::InvalidClock)?;
    require!(elapsed >= 0, VaultError::CliffActive);
    u64::try_from(elapsed / PERIOD_SECONDS).map_err(|_| error!(VaultError::ArithmeticOverflow))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{STATE_SEED, TOKEN_VAULT_SEED};
    use anchor_lang::prelude::Pubkey;

    #[test]
    fn cap_uses_two_explicit_floor_operations() {
        assert_eq!(monthly_cap(1_000_000_000, 500).unwrap(), 4_166_666);
        assert_eq!(monthly_cap(239, 500).unwrap(), 0);
    }

    #[test]
    fn periods_are_fixed_thirty_day_windows_after_cliff() {
        assert_eq!(period_index(1_000, 1_000).unwrap(), 0);
        assert_eq!(period_index(1_000 + PERIOD_SECONDS - 1, 1_000).unwrap(), 0);
        assert_eq!(period_index(1_000 + PERIOD_SECONDS, 1_000).unwrap(), 1);
        assert!(period_index(999, 1_000).is_err());
    }

    #[test]
    fn pda_vector_matches_the_standard_library_verifier() {
        let program_id = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 1));
        let beneficiary = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 33));
        let mint = Pubkey::new_from_array(std::array::from_fn(|index| index as u8 + 65));
        let policy_hash = [0x42; 32];
        let (state, state_bump) = Pubkey::find_program_address(
            &[
                STATE_SEED,
                beneficiary.as_ref(),
                mint.as_ref(),
                policy_hash.as_ref(),
            ],
            &program_id,
        );
        let (token, token_bump) =
            Pubkey::find_program_address(&[TOKEN_VAULT_SEED, state.as_ref()], &program_id);

        assert_eq!(
            state.to_string(),
            "3uGrhnqpmm7gC9s3sHoREzb8C1v9SNUXDwcHbyxbM7Hn"
        );
        assert_eq!(
            token.to_string(),
            "DrMcf5HTqEXHwXv15sghdR9zHcSYzvPEyARh2LkyPvYW"
        );
        assert_eq!((state_bump, token_bump), (255, 255));
    }
}
