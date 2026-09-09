use crate::*;

#[derive(Accounts)]
pub struct InitializeGate<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub member_one: Signer<'info>,
    pub member_two: Signer<'info>,
    pub member_three: Signer<'info>,
    pub current_authority: Signer<'info>,
    /// CHECK: raw executable loader Program and canonical ProgramData validated in handler.
    pub target: UncheckedAccount<'info>,
    /// CHECK: raw loader owner, state and current authority checked before transfer.
    #[account(mut)]
    pub programdata: UncheckedAccount<'info>,
    /// CHECK: canonical loader ProgramData of this gate must have authority None.
    pub gate_programdata: UncheckedAccount<'info>,
    #[account(init, payer = payer, space = 8 + UpgradeGateV1::INIT_SPACE,
        seeds = [b"upgrade-gate-v1", target.key().as_ref()], bump)]
    pub gate: Account<'info, UpgradeGateV1>,
    /// CHECK: fixed native loader program.
    #[account(address = anchor_lang::solana_program::bpf_loader_upgradeable::ID, executable)]
    pub loader: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ProposeUpgrade<'info> {
    pub member_one: Signer<'info>,
    pub member_two: Signer<'info>,
    pub return_authority: Signer<'info>,
    #[account(mut, seeds = [b"upgrade-gate-v1", gate.target.as_ref()], bump = gate.bump)]
    pub gate: Account<'info, UpgradeGateV1>,
    /// CHECK: raw Buffer owner, authority, exact bytes and hash checked in handler.
    pub buffer: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ExecuteUpgrade<'info> {
    #[account(mut, seeds = [b"upgrade-gate-v1", gate.target.as_ref()], bump = gate.bump)]
    pub gate: Account<'info, UpgradeGateV1>,
    /// CHECK: immutable target binding plus raw loader state checked in handler.
    #[account(mut, address = gate.target)]
    pub target: UncheckedAccount<'info>,
    /// CHECK: canonical ProgramData and gate PDA authority checked in handler.
    #[account(mut, address = gate.programdata)]
    pub programdata: UncheckedAccount<'info>,
    /// CHECK: pending buffer, loader authority, length and hash checked in handler.
    #[account(mut)]
    pub buffer: UncheckedAccount<'info>,
    /// CHECK: destination is the accepted, stored buffer return authority.
    #[account(mut)]
    pub spill: UncheckedAccount<'info>,
    pub rent: Sysvar<'info, Rent>,
    pub clock: Sysvar<'info, Clock>,
    /// CHECK: fixed native loader program.
    #[account(address = anchor_lang::solana_program::bpf_loader_upgradeable::ID, executable)]
    pub loader: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ReturnBuffer<'info> {
    pub member_one: Signer<'info>,
    pub member_two: Signer<'info>,
    #[account(mut, seeds = [b"upgrade-gate-v1", gate.target.as_ref()], bump = gate.bump)]
    pub gate: Account<'info, UpgradeGateV1>,
    /// CHECK: gate-owned loader Buffer checked; queued buffer has stricter cancel path.
    #[account(mut)]
    pub buffer: UncheckedAccount<'info>,
    /// CHECK: cancel checks stored return authority; unqueued recovery is quorum-authorized.
    pub recipient: UncheckedAccount<'info>,
    /// CHECK: fixed native loader program.
    #[account(address = anchor_lang::solana_program::bpf_loader_upgradeable::ID, executable)]
    pub loader: UncheckedAccount<'info>,
}
