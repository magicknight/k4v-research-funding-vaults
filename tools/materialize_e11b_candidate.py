#!/usr/bin/env python3
"""One-time, auditable E11B derivation. Does not edit any parent artifact.
Source inputs are the frozen v6 tree; existing candidates may not be overwritten.
No network, private keys, public-chain transactions or production choices.
"""
from pathlib import Path
import hashlib
import json
import re
import struct
import subprocess

R = Path(__file__).resolve().parents[1]
assert subprocess.check_output(['git', 'rev-parse', 'HEAD:candidates/launch-vault-v6'], cwd=R, text=True).strip() == '8e060ca354e6a941dcd80a40095d91fada6595ff', 'FROZEN_V6_TREE_CHANGED'
subprocess.run(['git', 'diff', '--exit-code', '--', 'candidates/launch-vault-v6'], cwd=R, check=True)
OLD_ID = 'FixSiDfTxvoy5Zgjp5KdFU8U23ChwCxPWY3WTkmMW2fU'
NEW_ID = 'CYFsfATtQB3Excjsm4Cuh8ZWnPE5j6XAU3GS3RKXmUcK'
PUB = bytes.fromhex('ab7260f20edab8208990343f0b9954b20b42b0bd81c8256bebab0c70d41750cc')

def norm(s):
    for a, b in [(OLD_ID, NEW_ID), ('launch-vault-v6', 'launch-vault-v7'),
                 ('launch_vault_v6', 'launch_vault_v7'), ('launch_v6', 'launch_v7'),
                 ('LAUNCH_V6', 'LAUNCH_V7'), ('launch-v6', 'launch-v7'),
                 ('V6', 'V7'), ('v6', 'v7'), ('E-10', 'E-11B'), ('E10', 'E11B')]:
        s = s.replace(a, b)
    return re.sub(r'(?<![a-z0-9])e10(?![a-z0-9])', 'e11b', s)

D = R / 'candidates/launch-vault-v7'
assert not D.exists(), 'CANDIDATE_ALREADY_EXISTS_DO_NOT_OVERWRITE'
D.mkdir()
for source in (R / 'candidates/launch-vault-v6').rglob('*'):
    if source.is_file():
        dest = D / source.relative_to(R / 'candidates/launch-vault-v6')
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(norm(source.read_text()))
p = D / 'tests/support/e10.rs'
p.rename(D / 'tests/support/e11b.rs')
p = D / 'tests/support/e11b.rs'
p.write_text(p.read_text().replace('[88; 32]', '[89; 32]').replace(
    '("policy".to_string(), f.policy),',
    '("policy".to_string(), f.policy),\n        ("preparation".into(), f.preparation()),'))

p = D / 'src/lib.rs'
s = p.read_text()
a = s.index('    pub fn open_policy(')
b = s.index('    pub fn deposit(', a)
initializer = s[s.index('        ctx.accounts.policy.set_inner(', a):s.index('        Ok(())', a)]
new = '''    /// Records immutable intent only; no policy or token rights exist yet.
    pub fn prepare_policy(
        ctx: Context<PreparePolicy>,
        config: LaunchConfig,
        spec_hash: [u8; 32],
        identity: [u8; 32],
    ) -> Result<()> {
        let preparation = LaunchPreparationV7 {
            creator: ctx.accounts.creator.key(), mint: ctx.accounts.mint.key(),
            founder: ctx.accounts.founder.key(), treasury: ctx.accounts.treasury.key(),
            oracle: ctx.accounts.oracle.key(), identity, spec_hash, config,
            bump: ctx.bumps.preparation,
        };
        bootstrap::validate(&preparation, &ctx.accounts.mint, Clock::get()?.unix_timestamp)?;
        ctx.accounts.preparation.set_inner(preparation);
        Ok(())
    }

    /// All designated roles consent to the immutable preparation, without config arguments.
    pub fn open_prepared_policy(ctx: Context<OpenPreparedPolicy>) -> Result<()> {
        let preparation = &ctx.accounts.preparation;
        let now = Clock::get()?.unix_timestamp;
        bootstrap::validate(preparation, &ctx.accounts.mint, now)?;
        require!([preparation.creator, preparation.mint, preparation.founder,
            preparation.treasury, preparation.oracle] == [ctx.accounts.creator.key(),
            ctx.accounts.mint.key(), ctx.accounts.founder.key(), ctx.accounts.treasury.key(),
            ctx.accounts.oracle.key()], LaunchError::IdentityMismatch);
        require!(preparation.config.recovery_keys == [ctx.accounts.recovery_one.key(),
            ctx.accounts.recovery_two.key(), ctx.accounts.recovery_three.key()], LaunchError::Unauthorized);
        let config = preparation.config;
        let identity = preparation.identity;
        let spec_hash = preparation.spec_hash;
''' + initializer + '''        Ok(())
    }

'''
s = s[:a] + new + s[b:]
p.write_text(s.replace('pub mod capacity;', 'pub mod bootstrap;\npub mod capacity;'))
(D / 'src/bootstrap.rs').write_text('''//! TEST_ONLY immutable preparation admission; no transfers or mutable config API.
use crate::{capacity, governance, state::*};
use anchor_lang::prelude::*;
use anchor_spl::token::Mint;

pub fn validate(p: &LaunchPreparationV7, mint: &Mint, now: i64) -> Result<()> {
    require!(cfg!(feature = "test-profile"), LaunchError::ExperimentalProfileDisabled);
    let config = &p.config;
    require!(config.t0 > now, LaunchError::T0Boundary);
    config.t0.checked_add(CLIFF).ok_or(LaunchError::Overflow)?;
    capacity::validate_rules(config)?;
    governance::validate_recovery_keys(&config.recovery_keys)?;
    for (keys, actor) in [(&config.founder_recovery_keys, p.founder),
        (&config.treasury_recovery_keys, p.treasury)] {
        governance::validate_recovery_keys(keys)?;
        require!(!keys.contains(&actor), LaunchError::InvalidConfig);
    }
    require!(config.founder_amount.checked_add(config.treasury_amount).ok_or(LaunchError::Overflow)?
        <= mint.supply, LaunchError::InvalidConfig);
    require!(config.founder_amount > 0 && config.treasury_amount > 0
        && config.founder_period_cap > 0 && config.founder_period_cap <= config.founder_amount
        && config.treasury_period_cap > 0 && config.treasury_period_cap <= config.treasury_amount
        && config.shared_hard_cap > 0 && (1..=7 * 86_400).contains(&config.max_report_age)
        && p.spec_hash != [0; 32] && p.oracle != Pubkey::default(), LaunchError::InvalidConfig);
    require!(mint.mint_authority.is_none() && mint.freeze_authority.is_none(), LaunchError::MintAuthorityLive);
    require!(identity(&p.creator, &p.mint, &p.founder, &p.treasury, &p.oracle, &p.spec_hash, config)
        == p.identity, LaunchError::IdentityMismatch);
    Ok(())
}
''')
p = D / 'src/state.rs'
s = p.read_text()
i = s.index('#[account]')
s = s[:i] + '''/// Immutable content-addressed intent. No update or close instruction exists.
/// Unused preparations retain rent; changed configuration requires a new PDA.
#[account]
#[derive(InitSpace)]
pub struct LaunchPreparationV7 {
    pub creator: Pubkey,
    pub mint: Pubkey,
    pub founder: Pubkey,
    pub treasury: Pubkey,
    pub oracle: Pubkey,
    pub identity: [u8; 32],
    pub spec_hash: [u8; 32],
    pub config: LaunchConfig,
    pub bump: u8,
}

''' + s[i:]
p.write_text(s)
p = D / 'src/contexts.rs'
s = p.read_text()
a = s.index('#[derive(Accounts)]')
b = s.index('#[derive(Accounts)]', a + 1)
s = s[:a] + '''#[derive(Accounts)]
#[instruction(config: LaunchConfig, spec_hash: [u8; 32], identity: [u8; 32])]
pub struct PreparePolicy<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    /// CHECK: bound into immutable intent; must sign subsequent confirmation.
    pub founder: UncheckedAccount<'info>,
    /// CHECK: bound into immutable intent; must sign subsequent confirmation.
    pub treasury: UncheckedAccount<'info>,
    /// CHECK: identity only; subsequent reports require the frozen oracle signature.
    pub oracle: UncheckedAccount<'info>,
    pub mint: Account<'info, Mint>,
    #[account(init, payer = creator, space = 8 + LaunchPreparationV7::INIT_SPACE,
        seeds = [b"launch-v7-preparation", identity.as_ref()], bump)]
    pub preparation: Box<Account<'info, LaunchPreparationV7>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OpenPreparedPolicy<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    pub founder: Signer<'info>,
    pub treasury: Signer<'info>,
    /// CHECK: equality to the immutable preparation is verified by the handler.
    pub oracle: UncheckedAccount<'info>,
    pub recovery_one: Signer<'info>,
    pub recovery_two: Signer<'info>,
    pub recovery_three: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(seeds = [b"launch-v7-preparation", preparation.identity.as_ref()], bump = preparation.bump)]
    pub preparation: Box<Account<'info, LaunchPreparationV7>>,
    #[account(init, payer = creator, space = 8 + LaunchPolicyV7::INIT_SPACE,
        seeds = [b"launch-v7-policy", preparation.identity.as_ref()], bump)]
    pub policy: Box<Account<'info, LaunchPolicyV7>>,
    pub system_program: Program<'info, System>,
}

''' + s[b:]
p.write_text(s)

# Inherited tests now send a separate signed preparation before compact confirmation.
p = D / 'tests/launch_litesvm.rs'
s = p.read_text().replace('instruction::OpenPolicy', 'instruction::OpenPreparedPolicy').replace('accounts::OpenPolicy', 'accounts::OpenPreparedPolicy')
s = s.replace('        let result = self.run_many(vec![instruction], self.creator.pubkey());', '''        if opening && self.svm.get_account(&self.preparation()).is_none() {
            self.run_many(vec![self.prepare_ix()], self.creator.pubkey())?;
        }
        let result = self.run_many(vec![instruction], self.creator.pubkey());''', 1)
a = s.index('    fn open_ix(&self)')
b = s.index('    fn vault(', a)
s = s[:a] + '''    fn preparation(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"launch-v7-preparation", &self.hash], &ID).0
    }

    fn prepare_ix(&self) -> Instruction {
        ix(accounts::PreparePolicy {
            creator: self.creator.pubkey(), founder: self.founder.pubkey(), treasury: self.treasury.pubkey(),
            oracle: self.oracle.pubkey(), mint: self.mint, preparation: self.preparation(),
            system_program: solana_system_interface::program::ID,
        }, instruction::PreparePolicy { config: self.config, spec_hash: [42; 32], identity: self.hash })
    }

    fn open_ix(&self) -> Instruction {
        ix(accounts::OpenPreparedPolicy {
            creator: self.creator.pubkey(), founder: self.founder.pubkey(), treasury: self.treasury.pubkey(),
            oracle: self.oracle.pubkey(), recovery_one: self.recovery[0].pubkey(),
            recovery_two: self.recovery[1].pubkey(), recovery_three: self.recovery[2].pubkey(),
            mint: self.mint, preparation: self.preparation(), policy: self.policy,
            system_program: solana_system_interface::program::ID,
        }, instruction::OpenPreparedPolicy {})
    }

''' + s[b:]
s = s.replace('    f.reject_unchanged(stolen, "IdentityMismatch");', '    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();\n    f.reject_unchanged(stolen, "IdentityMismatch");', 1)
s = s.replace('    f.reject_unchanged(f.open_ix(), "IdentityMismatch");\n    f.config.t0 -= 1;', '    f.reject_unchanged(f.prepare_ix(), "already in use");\n    f.config.t0 -= 1;', 1)
s = s.replace('    let mut unsigned = f.open_ix();', '    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();\n    let mut unsigned = f.open_ix();', 1)
a = s.index('fn annual_inputs_are_immutable_bound_and_reject_invalid_ranges_rates_or_sources()')
b = s.index('\n#[test]', a)
s = s[:a] + s[a:b].replace('f.open_ix()', 'f.prepare_ix()') + s[b:]
s = s.replace('    let mut missing = f.open_ix();', '    f.run_many(vec![f.prepare_ix()], f.creator.pubkey()).unwrap();\n    let mut missing = f.open_ix();', 1)
s = s.replace('    f.config.recovery_keys.swap(0, 1);\n    assert!(f.run(f.open_ix()).is_err());', '    f.recovery.swap(0, 1);\n    assert!(f.run(f.open_ix()).is_err());', 1)
p.write_text(s)
p = D / 'tests/abi.rs'
s = p.read_text().replace('LaunchVaultV7, TreasuryApprovalV7,', 'LaunchVaultV7, LaunchPreparationV7, TreasuryApprovalV7,')
s = s.replace('    for (name, discriminator, bytes) in [', '    for (name, discriminator, bytes) in [\n        ("LaunchPreparationV7", LaunchPreparationV7::DISCRIMINATOR, LaunchPreparationV7::INIT_SPACE),')
p.write_text(s)
p = D / 'tests/submission_window.rs'
s = p.read_text()
a = s.index('        let opening = ix(')
b = s.index('        let mut p = Self', a)
s = s[:a] + '''        let preparation = Pubkey::find_program_address(&[b"launch-v7-preparation", &identity], &ID).0;
        let prepare = ix(accounts::PreparePolicy {
            creator: keys[0].pubkey(), founder: keys[1].pubkey(), treasury: keys[2].pubkey(),
            oracle: keys[3].pubkey(), mint, preparation,
            system_program: solana_system_interface::program::ID,
        }, instruction::PreparePolicy { config, spec_hash: [42; 32], identity });
        let opening = ix(accounts::OpenPreparedPolicy {
            creator: keys[0].pubkey(), founder: keys[1].pubkey(), treasury: keys[2].pubkey(),
            oracle: keys[3].pubkey(), recovery_one: keys[4].pubkey(), recovery_two: keys[5].pubkey(),
            recovery_three: keys[6].pubkey(), mint, preparation, policy,
            system_program: solana_system_interface::program::ID,
        }, instruction::OpenPreparedPolicy {});
''' + s[b:]
s = s.replace('        p.time(NOW);\n        let tx = p.sign(opening);', '        p.time(NOW);\n        let tx = p.sign(prepare);\n        p.svm.send_transaction(tx).unwrap();\n        let tx = p.sign(opening);')
p.write_text(s)

for rel in ['tools/build_launch_v6_idl.py', 'probes/launch_v6_identity.mjs',
            'probes/launch_v6_identity.test.mjs', 'src/launch_v6_verifier.py',
            'src/launch_v6_rpc_exporter.py', 'src/e10_verifier.py',
            'tools/pack_e10_rehearsal.py', 'tools/e10_recorded_rpc.py', 'tools/verify_e10_probes.py']:
    (R / norm(rel)).write_text(norm((R / rel).read_text()))
p = R / 'tools/build_launch_v7_idl.py'
p.write_text(p.read_text().replace('== 18', '== 19').replace('== 6', '== 7'))
v = json.loads((R / 'spec/LAUNCH_V6_IDENTITY_VECTOR_v1.json').read_text())
v['program_hex'] = PUB.hex()
v['profile'] = 'TEST_ONLY_IMMUTABLE_BOOTSTRAP_1'
constants = struct.pack('<qqQHqBqq', 15552000, 2592000, 12, 500, 7776000, 2, 2592000, 300)
v['identity_hex'] = hashlib.sha256(b'k4v-launch-policy-v7-test-profile-1' + b''.join(bytes.fromhex(v[k]) for k in ['program_hex', 'creator_hex', 'mint_hex', 'founder_hex', 'treasury_hex', 'oracle_hex', 'specHash_hex']) + constants + bytes.fromhex(v['config_borsh_hex'])).hexdigest()
(R / 'spec/LAUNCH_V7_IDENTITY_VECTOR_v1.json').write_text(json.dumps(v, indent=2) + '\n')
(R / 'idl/launch_vault_v7.json').write_text('{}\n')
print('Isolated v7 source materialized; compilation and acceptance have NOT yet run.')
