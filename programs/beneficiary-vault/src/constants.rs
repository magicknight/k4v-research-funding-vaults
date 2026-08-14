pub const BPS_DENOMINATOR: u128 = 10_000;
pub const MONTHS_PER_YEAR: u128 = 12;
pub const MAX_ANNUAL_RELEASE_BPS: u16 = 500;
pub const MIN_CLIFF_SECONDS: i64 = 730 * 24 * 60 * 60;
pub const PERIOD_SECONDS: i64 = 30 * 24 * 60 * 60;

pub const STATE_SEED: &[u8] = b"beneficiary-vault";
pub const TOKEN_VAULT_SEED: &[u8] = b"beneficiary-token";
