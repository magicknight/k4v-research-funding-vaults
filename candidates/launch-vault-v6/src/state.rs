use anchor_lang::prelude::*;

pub const PERIOD: i64 = 30 * 86_400;
pub const CLIFF: i64 = 180 * 86_400;
pub const PREPARED: u8 = 0;
pub const ARMED: u8 = 1;
pub const ACTIVE: u8 = 2;
pub const CANCELLED: u8 = 3;
pub const FOUNDER: u8 = 0;
pub const TREASURY: u8 = 1;
pub const RATE_DIVISOR: u64 = 12;
pub const MAX_RELEASE_BPS: u16 = 500;
pub const CHANGE_NOTICE: i64 = 90 * 86_400;
pub const CHANGE_ORACLE: u8 = 0;
pub const CHANGE_CONTROLLER: u8 = 1;
pub const PROPOSAL_PENDING: u8 = 0;
pub const PROPOSAL_EXECUTED: u8 = 1;
pub const PROPOSAL_CANCELLED: u8 = 2;
pub const PROPOSAL_EXPIRED: u8 = 3;
pub const MAX_SUBMISSION_WINDOW: i64 = 300;
pub const WITHDRAWAL_EXECUTION_WINDOW: i64 = 30 * 86_400;

/// Two frozen input epochs exercise the annual interface. They are not a
/// production calendar, live IRB source, or authority to extend the horizon.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace, Debug)]
pub struct AnnualRule {
    pub start_period: u64,
    pub end_period: u64,
    pub founder_basis: u64,
    pub treasury_basis: u64,
    pub shared_cap: u64,
    pub release_bps: u16,
    pub source_hash: [u8; 32],
}

/// All amounts are mint base units. Caps are TEST_ONLY fixtures, not annual IRB.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace, Debug)]
pub struct LaunchConfig {
    pub t0: i64,
    pub founder_amount: u64,
    pub treasury_amount: u64,
    pub founder_period_cap: u64,
    pub treasury_period_cap: u64,
    pub shared_hard_cap: u64,
    pub max_report_age: i64,
    pub annual_rules: [AnnualRule; 2],
    pub recovery_keys: [Pubkey; 3],
    pub founder_recovery_keys: [Pubkey; 3],
    pub treasury_recovery_keys: [Pubkey; 3],
}

#[account]
#[derive(InitSpace)]
pub struct LaunchPolicyV6 {
    pub creator: Pubkey,
    pub mint: Pubkey,
    pub founder: Pubkey,
    pub treasury: Pubkey,
    pub oracle: Pubkey,
    pub identity: [u8; 32],
    pub spec_hash: [u8; 32],
    pub config: LaunchConfig,
    pub state: u8,
    pub funded_mask: u8,
    pub bump: u8,
    pub last_action_at: i64,
    pub period: u64,
    pub shared_used: u64,
    pub report_period: u64,
    pub report_capacity: u64,
    pub report_at: i64,
    pub report_sequence: u64,
    pub founder_period_used: u64,
    pub treasury_period_used: u64,
    pub founder_released_total: u64,
    pub treasury_released_total: u64,
    pub annual_index: u8,
    pub founder_annual_used: u64,
    pub treasury_annual_used: u64,
    // Initial actors remain immutable identity inputs. Current control is separate.
    pub initial_oracle: Pubkey,
    pub controller: Pubkey,
    pub controller_epoch: u64,
    pub oracle_epoch: u64,
    pub oracle_activated_at: i64,
    pub report_epoch: u64,
    pub report_valid: bool,
    pub change_sequence: u64,
    pub pending_change: u64,
    pub withdrawal: [WithdrawalRole; 2],
    pub approval_count: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace, Debug)]
pub struct WithdrawalRole {
    pub current: Pubkey,
    pub epoch: u64,
    pub sequence: u64,
    pub pending: u64,
}

#[account]
#[derive(InitSpace)]
pub struct WithdrawalProposalV6 {
    pub policy: Pubkey,
    pub role: u8,
    pub nonce: u64,
    pub epoch: u64,
    pub predecessor: Pubkey,
    pub successor: Pubkey,
    pub recovery: bool,
    pub valid_from: i64,
    pub valid_until: i64,
    pub created_at: i64,
    pub execute_after: i64,
    pub expires_at: i64,
    pub status: u8,
    pub finished_at: i64,
    pub bump: u8,
}

/// Permanent O(1) canonical key index. No reset/close instruction exists.
/// Initial actors and guardians are also checked directly from frozen config.
#[account]
#[derive(InitSpace)]
pub struct WithdrawalKeyV6 {
    pub policy: Pubkey,
    pub subject: Pubkey,
    pub history_mask: u8,
    pub pending_mask: u8,
    pub recipient: bool,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct LaunchVaultV6 {
    pub policy: Pubkey,
    pub depositor: Pubkey,
    pub authority: Pubkey,
    pub role: u8,
    pub bump: u8,
    pub principal: u64,
    pub released_total: u64,
    pub period: u64,
    pub period_used: u64,
}

#[account]
#[derive(InitSpace)]
pub struct TreasuryApprovalV6 {
    pub policy: Pubkey,
    pub period: u64,
    pub recipient: Pubkey,
    pub recipient_owner: Pubkey,
    pub need: u64,
    pub consumed: u64,
    pub created_at: i64,
    pub bump: u8,
    pub author: Pubkey,
    pub author_epoch: u64,
}

#[account]
#[derive(InitSpace)]
pub struct ChangeProposalV6 {
    pub policy: Pubkey,
    pub nonce: u64,
    pub kind: u8,
    pub recovery: bool,
    pub successor: Pubkey,
    pub controller_epoch: u64,
    pub oracle_epoch: u64,
    pub created_at: i64,
    pub execute_after: i64,
    pub status: u8,
    pub bump: u8,
}

#[error_code]
pub enum LaunchError {
    #[msg("This build cannot admit launch policies; no production profile is approved")]
    ExperimentalProfileDisabled,
    #[msg("Invalid immutable configuration or role")]
    InvalidConfig,
    #[msg("Policy identity does not bind these actors, mint and exact configuration")]
    IdentityMismatch,
    #[msg("Operation is unavailable in this lifecycle state")]
    InvalidState,
    #[msg("T0 boundary has not been satisfied")]
    T0Boundary,
    #[msg("Both designated pools must be funded exactly once")]
    PoolsNotFunded,
    #[msg("Unauthorized actor, account or destination")]
    Unauthorized,
    #[msg("Mint and freeze authorities must both be revoked")]
    MintAuthorityLive,
    #[msg("Deposit must equal the configured principal")]
    WrongPrincipal,
    #[msg("Clock or period moved backwards")]
    ClockRollback,
    #[msg("Founder 180-day cliff remains active")]
    CliffActive,
    #[msg("Capacity report is missing, stale, replayed or invalid")]
    InvalidReport,
    #[msg("Amount exceeds principal, period cap or shared capacity")]
    CapacityExceeded,
    #[msg("Treasury approval, recipient, period or notice is invalid")]
    InvalidApproval,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Amount must be nonzero")]
    ZeroAmount,
    #[msg("No frozen annual rule covers this period, or its input is invalid")]
    InvalidAnnualRule,
    #[msg("A stored accounting invariant was violated")]
    InvalidAccounting,
    #[msg("A corrected quota is below already-used capacity; both pools pause")]
    CapacityCorrectionPause,
    #[msg("This pool cannot consume the other pool's reserved capacity")]
    ReservedQuotaExceeded,
    #[msg("A different proposal is pending, or the nonce/epoch was replayed")]
    InvalidChange,
    #[msg("The accepted change is not mature")]
    ChangeNotice,
    #[msg("Two distinct pre-registered recovery keys are required")]
    RecoveryQuorum,
    #[msg("Beneficiary role is paused by a pending accepted change")]
    WithdrawalPaused,
    #[msg("Beneficiary key or authority epoch is not current")]
    WithdrawalAuthority,
    #[msg("Known beneficiary/guardian/successor cannot receive Treasury payments")]
    KnownSelfPayment,
    #[msg("Retired keys, own guardians and historical recipients cannot be successors")]
    IneligibleSuccessor,
    #[msg("Signed admission interval is invalid or runtime Clock lies outside it")]
    SubmissionWindow,
    #[msg("Withdrawal proposal is outside its execution or expiry window")]
    WithdrawalWindow,
}

pub fn identity(
    creator: &Pubkey,
    mint: &Pubkey,
    founder: &Pubkey,
    treasury: &Pubkey,
    oracle: &Pubkey,
    spec_hash: &[u8; 32],
    config: &LaunchConfig,
) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(300);
    config
        .serialize(&mut bytes)
        .expect("fixed-size Borsh configuration");
    solana_sha256_hasher::hashv(&[
        b"k4v-launch-policy-v6-test-profile-1",
        crate::ID.as_ref(),
        creator.as_ref(),
        mint.as_ref(),
        founder.as_ref(),
        treasury.as_ref(),
        oracle.as_ref(),
        spec_hash,
        &CLIFF.to_le_bytes(),
        &PERIOD.to_le_bytes(),
        &RATE_DIVISOR.to_le_bytes(),
        &MAX_RELEASE_BPS.to_le_bytes(),
        &CHANGE_NOTICE.to_le_bytes(),
        &[2u8],
        &WITHDRAWAL_EXECUTION_WINDOW.to_le_bytes(),
        &MAX_SUBMISSION_WINDOW.to_le_bytes(),
        &bytes,
    ])
    .to_bytes()
}

pub fn period_at(t0: i64, now: i64) -> Result<u64> {
    require!(now >= t0, LaunchError::T0Boundary);
    let elapsed = now.checked_sub(t0).ok_or(LaunchError::Overflow)?;
    Ok((elapsed / PERIOD) as u64)
}

pub fn period_start(t0: i64, period: u64) -> Result<i64> {
    let result = i128::from(t0) + i128::from(period) * i128::from(PERIOD);
    i64::try_from(result).map_err(|_| error!(LaunchError::Overflow))
}

impl LaunchPolicyV6 {
    pub fn withdrawal_for(&self, role: u8) -> Result<&WithdrawalRole> {
        self.withdrawal
            .get(usize::from(role))
            .ok_or_else(|| error!(LaunchError::InvalidConfig))
    }

    pub fn authenticate_withdrawal(&self, role: u8, signer: Pubkey, epoch: u64) -> Result<()> {
        let r = self.withdrawal_for(role)?;
        require!(r.pending == 0, LaunchError::WithdrawalPaused);
        require!(
            r.current == signer && r.epoch == epoch,
            LaunchError::WithdrawalAuthority
        );
        Ok(())
    }

    pub fn withdrawal_keys(&self, role: u8) -> Result<&[Pubkey; 3]> {
        match role {
            FOUNDER => Ok(&self.config.founder_recovery_keys),
            TREASURY => Ok(&self.config.treasury_recovery_keys),
            _ => err!(LaunchError::InvalidConfig),
        }
    }
    pub fn tick(&mut self) -> Result<i64> {
        let now = Clock::get()?.unix_timestamp;
        require!(now >= self.last_action_at, LaunchError::ClockRollback);
        self.last_action_at = now;
        Ok(now)
    }

    pub fn owner_for(&self, role: u8) -> Result<Pubkey> {
        match role {
            FOUNDER => Ok(self.founder),
            TREASURY => Ok(self.treasury),
            _ => err!(LaunchError::InvalidConfig),
        }
    }

    pub fn principal_for(&self, role: u8) -> Result<u64> {
        match role {
            FOUNDER => Ok(self.config.founder_amount),
            TREASURY => Ok(self.config.treasury_amount),
            _ => err!(LaunchError::InvalidConfig),
        }
    }
}
