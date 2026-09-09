//! TEST_ONLY external upgrade gate. Initialization requires the gate itself
//! to have been sealed under the real upgradeable loader (authority = None).
use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::{invoke, invoke_signed};
use solana_loader_v3_interface::{
    instruction as loader, state::UpgradeableLoaderState as LoaderState,
};
pub mod contexts;
pub use contexts::*;

declare_id!("5JVsAM5AnTBdeHsGjC78hcrrv9KfLKVgCNN4RzBiWjmA");
pub const NOTICE: i64 = 90 * 86_400;
pub const EMPTY: u8 = 0;
pub const PENDING: u8 = 1;
pub const EXECUTED: u8 = 2;
pub const CANCELLED: u8 = 3;

#[account]
#[derive(InitSpace)]
pub struct UpgradeGateV1 {
    pub target: Pubkey,
    pub programdata: Pubkey,
    pub members: [Pubkey; 3],
    pub nonce: u64,
    pub status: u8,
    pub buffer: Pubkey,
    pub code_hash: [u8; 32],
    pub code_len: u64,
    pub return_authority: Pubkey,
    pub created_at: i64,
    pub execute_after: i64,
    pub last_action_at: i64,
    pub bump: u8,
}

#[error_code]
pub enum GateError {
    #[msg("Default artifact cannot initialize a TEST_ONLY gate")]
    Disabled,
    #[msg("Invalid distinct committee keys or target")]
    InvalidConfig,
    #[msg("Two distinct registered committee signatures are required")]
    Unauthorized,
    #[msg("Incorrect loader owner, target, programdata or authority")]
    InvalidLoader,
    #[msg("This gate must itself be immutable before it can control a target")]
    UnsealedGate,
    #[msg("Buffer authority, exact code length or hash does not match")]
    InvalidBuffer,
    #[msg("Pending proposal or nonce mismatch")]
    InvalidProposal,
    #[msg("The on-chain notice has not matured")]
    Notice,
    #[msg("Clock moved backwards")]
    ClockRollback,
    #[msg("Arithmetic overflow")]
    Overflow,
}

fn loader_state(info: &AccountInfo) -> Result<LoaderState> {
    require_keys_eq!(
        *info.owner,
        anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        GateError::InvalidLoader
    );
    bincode::deserialize(&info.try_borrow_data()?).map_err(|_| error!(GateError::InvalidLoader))
}

fn target_state(
    program: &AccountInfo,
    data: &AccountInfo,
    authority: Option<Pubkey>,
) -> Result<()> {
    require!(
        program.executable && !data.executable,
        GateError::InvalidLoader
    );
    require_keys_eq!(
        solana_loader_v3_interface::get_program_data_address(program.key),
        *data.key,
        GateError::InvalidLoader
    );
    require!(
        matches!(loader_state(program)?, LoaderState::Program { programdata_address }
        if programdata_address == *data.key),
        GateError::InvalidLoader
    );
    require!(
        matches!(loader_state(data)?, LoaderState::ProgramData { upgrade_authority_address, .. }
        if upgrade_authority_address == authority),
        GateError::InvalidLoader
    );
    Ok(())
}

fn buffer_hash(info: &AccountInfo, authority: Pubkey) -> Result<([u8; 32], u64)> {
    require!(!info.executable, GateError::InvalidBuffer);
    require!(
        matches!(loader_state(info)?, LoaderState::Buffer { authority_address: Some(a) }
        if a == authority),
        GateError::InvalidBuffer
    );
    let data = info.try_borrow_data()?;
    let bytes = data
        .get(LoaderState::size_of_buffer_metadata()..)
        .ok_or(GateError::InvalidBuffer)?;
    require!(!bytes.is_empty(), GateError::InvalidBuffer);
    Ok((
        solana_sha256_hasher::hash(bytes).to_bytes(),
        bytes.len() as u64,
    ))
}

impl UpgradeGateV1 {
    fn quorum(&self, a: Pubkey, b: Pubkey) -> Result<()> {
        require!(
            a != b && self.members.contains(&a) && self.members.contains(&b),
            GateError::Unauthorized
        );
        Ok(())
    }
    fn tick(&mut self) -> Result<i64> {
        let now = Clock::get()?.unix_timestamp;
        require!(now >= self.last_action_at, GateError::ClockRollback);
        self.last_action_at = now;
        Ok(now)
    }
}

#[program]
pub mod upgrade_gate_v1 {
    use super::*;

    pub fn initialize_gate(ctx: Context<InitializeGate>) -> Result<()> {
        require!(cfg!(feature = "test-profile"), GateError::Disabled);
        let members = [
            ctx.accounts.member_one.key(),
            ctx.accounts.member_two.key(),
            ctx.accounts.member_three.key(),
        ];
        require!(
            members[0] != members[1] && members[0] != members[2] && members[1] != members[2],
            GateError::InvalidConfig
        );
        require!(
            ctx.accounts.target.key() != crate::ID,
            GateError::InvalidConfig
        );
        // The programdata PDA and None authority are checked from raw loader state.
        require_keys_eq!(
            ctx.accounts.gate_programdata.key(),
            solana_loader_v3_interface::get_program_data_address(&crate::ID),
            GateError::UnsealedGate
        );
        require!(
            !ctx.accounts.gate_programdata.executable,
            GateError::UnsealedGate
        );
        require!(
            matches!(
                loader_state(&ctx.accounts.gate_programdata)?,
                LoaderState::ProgramData {
                    upgrade_authority_address: None,
                    ..
                }
            ),
            GateError::UnsealedGate
        );
        target_state(
            &ctx.accounts.target,
            &ctx.accounts.programdata,
            Some(ctx.accounts.current_authority.key()),
        )?;
        let now = Clock::get()?.unix_timestamp;
        ctx.accounts.gate.set_inner(UpgradeGateV1 {
            target: ctx.accounts.target.key(),
            programdata: ctx.accounts.programdata.key(),
            members,
            nonce: 0,
            status: EMPTY,
            buffer: Pubkey::default(),
            code_hash: [0; 32],
            code_len: 0,
            return_authority: Pubkey::default(),
            created_at: 0,
            execute_after: 0,
            last_action_at: now,
            bump: ctx.bumps.gate,
        });
        // No off-chain assertion substitutes for this real loader authority transfer.
        invoke(
            &loader::set_upgrade_authority(
                &ctx.accounts.target.key(),
                &ctx.accounts.current_authority.key(),
                Some(&ctx.accounts.gate.key()),
            ),
            &[
                ctx.accounts.programdata.to_account_info(),
                ctx.accounts.current_authority.to_account_info(),
                ctx.accounts.gate.to_account_info(),
                ctx.accounts.loader.to_account_info(),
            ],
        )?;
        Ok(())
    }

    pub fn propose_upgrade(
        ctx: Context<ProposeUpgrade>,
        nonce: u64,
        code_hash: [u8; 32],
    ) -> Result<()> {
        let g = &mut ctx.accounts.gate;
        let now = g.tick()?;
        g.quorum(ctx.accounts.member_one.key(), ctx.accounts.member_two.key())?;
        require!(
            g.status != PENDING && nonce == g.nonce.checked_add(1).ok_or(GateError::Overflow)?,
            GateError::InvalidProposal
        );
        let (actual, len) = buffer_hash(&ctx.accounts.buffer, g.key())?;
        require!(actual == code_hash, GateError::InvalidBuffer);
        g.nonce = nonce;
        g.status = PENDING;
        g.buffer = ctx.accounts.buffer.key();
        g.code_hash = actual;
        g.code_len = len;
        g.return_authority = ctx.accounts.return_authority.key();
        g.created_at = now;
        g.execute_after = now.checked_add(NOTICE).ok_or(GateError::Overflow)?;
        Ok(())
    }

    pub fn execute_upgrade(ctx: Context<ExecuteUpgrade>, nonce: u64) -> Result<()> {
        let g = &mut ctx.accounts.gate;
        let now = g.tick()?;
        require!(
            g.status == PENDING && g.nonce == nonce,
            GateError::InvalidProposal
        );
        require!(now >= g.execute_after, GateError::Notice);
        require_keys_eq!(
            g.buffer,
            ctx.accounts.buffer.key(),
            GateError::InvalidBuffer
        );
        require_keys_eq!(
            g.return_authority,
            ctx.accounts.spill.key(),
            GateError::InvalidProposal
        );
        target_state(
            &ctx.accounts.target,
            &ctx.accounts.programdata,
            Some(g.key()),
        )?;
        let (hash, len) = buffer_hash(&ctx.accounts.buffer, g.key())?;
        require!(
            hash == g.code_hash && len == g.code_len,
            GateError::InvalidBuffer
        );
        g.status = EXECUTED;
        let bump = [g.bump];
        let seeds: &[&[u8]] = &[b"upgrade-gate-v1", g.target.as_ref(), &bump];
        invoke_signed(
            &loader::upgrade(&g.target, &g.buffer, &g.key(), &g.return_authority),
            &[
                ctx.accounts.programdata.to_account_info(),
                ctx.accounts.target.to_account_info(),
                ctx.accounts.buffer.to_account_info(),
                ctx.accounts.spill.to_account_info(),
                ctx.accounts.rent.to_account_info(),
                ctx.accounts.clock.to_account_info(),
                g.to_account_info(),
                ctx.accounts.loader.to_account_info(),
            ],
            &[seeds],
        )?;
        Ok(())
    }

    pub fn cancel_upgrade(ctx: Context<ReturnBuffer>, nonce: u64) -> Result<()> {
        let g = &mut ctx.accounts.gate;
        g.tick()?;
        g.quorum(ctx.accounts.member_one.key(), ctx.accounts.member_two.key())?;
        require!(
            g.status == PENDING && g.nonce == nonce,
            GateError::InvalidProposal
        );
        require_keys_eq!(
            g.buffer,
            ctx.accounts.buffer.key(),
            GateError::InvalidBuffer
        );
        require_keys_eq!(
            g.return_authority,
            ctx.accounts.recipient.key(),
            GateError::InvalidProposal
        );
        buffer_hash(&ctx.accounts.buffer, g.key())?;
        g.status = CANCELLED;
        return_buffer(&ctx)
    }

    pub fn return_unqueued_buffer(ctx: Context<ReturnBuffer>) -> Result<()> {
        let g = &mut ctx.accounts.gate;
        g.tick()?;
        g.quorum(ctx.accounts.member_one.key(), ctx.accounts.member_two.key())?;
        require!(
            g.status != PENDING || g.buffer != ctx.accounts.buffer.key(),
            GateError::InvalidProposal
        );
        buffer_hash(&ctx.accounts.buffer, g.key())?;
        return_buffer(&ctx)
    }
}

fn return_buffer(ctx: &Context<ReturnBuffer>) -> Result<()> {
    let g = &ctx.accounts.gate;
    let bump = [g.bump];
    let seeds: &[&[u8]] = &[b"upgrade-gate-v1", g.target.as_ref(), &bump];
    invoke_signed(
        &loader::set_buffer_authority(
            &ctx.accounts.buffer.key(),
            &g.key(),
            &ctx.accounts.recipient.key(),
        ),
        &[
            ctx.accounts.buffer.to_account_info(),
            g.to_account_info(),
            ctx.accounts.recipient.to_account_info(),
            ctx.accounts.loader.to_account_info(),
        ],
        &[seeds],
    )?;
    Ok(())
}
