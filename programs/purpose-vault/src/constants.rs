pub const BPS_DENOMINATOR: u128 = 10_000;
pub const MONTHS_PER_YEAR: u128 = 12;
pub const MAX_ANNUAL_RELEASE_BPS: u16 = 500;
pub const MAX_MARKET_CAPACITY_BPS: u16 = 500;
pub const MIN_CLIFF_SECONDS: i64 = 730 * 24 * 60 * 60;
pub const PERIOD_SECONDS: i64 = 30 * 24 * 60 * 60;

/// A purpose approval must sit on chain this long before it can be consumed.
/// It mirrors the covenant's 30-day public-notice requirement.
pub const MIN_NOTICE_SECONDS: i64 = 30 * 24 * 60 * 60;

/// A policy may not declare a market input tolerance looser than this. Without
/// a ceiling, a deployment could set the tolerance to a century and call a
/// permanently stale oracle "fresh".
pub const MAX_INPUT_AGE_CEILING_SECONDS: i64 = 7 * 24 * 60 * 60;
/// A proposed oracle waits this long in public before it can take effect. It is
/// frozen rather than configurable: the delay is what makes the change visible
/// to holders before it binds, and a policy that could shorten it could remove
/// the visibility it exists to provide.
pub const ORACLE_ROTATION_NOTICE_SECONDS: i64 = 90 * 24 * 60 * 60;
/// A declared silence floor cannot engage sooner than this. It sits well beyond
/// the rotation notice on purpose: replacing a lost oracle is the repair, and
/// the floor is only for the case where the authority that would rotate it is
/// gone too.
pub const MIN_SILENCE_GRACE_SECONDS: i64 = 180 * 24 * 60 * 60;
pub const MAX_SILENCE_GRACE_SECONDS: i64 = 730 * 24 * 60 * 60;

pub const POLICY_SEED: &[u8] = b"purpose-policy";
pub const MARKET_SEED: &[u8] = b"purpose-market";
pub const VAULT_SEED: &[u8] = b"purpose-vault";
pub const TOKEN_VAULT_SEED: &[u8] = b"purpose-token";
pub const APPROVAL_SEED: &[u8] = b"purpose-approval";
