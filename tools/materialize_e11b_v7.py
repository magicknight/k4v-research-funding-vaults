#!/usr/bin/env python3
"""Deterministically materialize an isolated TEST_ONLY v7 from frozen v6 inputs.

The checked-in generator is source, not a receipt. A successful materialization
is not a compiled/tested SBF. This never writes a v6 source, production config,
keypair or chain account. Generated output must not preexist.
"""
from __future__ import annotations
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OLD = ROOT / 'candidates/launch-vault-v6'
NEW = ROOT / 'candidates/launch-vault-v7'
OLD_ID = 'FixSiDfTxvoy5Zgjp5KdFU8U23ChwCxPWY3WTkmMW2fU'
NEW_ID = 'CYFsfATtQB3Excjsm4Cuh8ZWnPE5j6XAU3GS3RKXmUcK'
# The new ID is a PUBLIC, INSECURE TEST FIXTURE ([89;32] Ed25519 seed).
# It must NEVER be used for a public deployment or real funds.
INPUTS = {
    'Cargo.toml': '7d6ac47cfafb37af8c87180fffa0160dcc8773ae',
    'Cargo.lock': '322dc563d5e3c09a602ac82e383f8153b1b274f1',
    'src/capacity.rs': '8f456f5ca1338981da9955ec3617fa5e10418fee',
    'src/contexts.rs': '7acfa75fa804f13c26b861494c49408c3f92e427',
    'src/governance.rs': 'e946ece783d6b94c0ea2a8e8ee7a89e0adfd250d',
    'src/lib.rs': 'df6bc2b84ea760a3c073eb7871b299e72418fa2c',
    'src/state.rs': 'b887e19facd2f96589899a817f8f1842f7cfb495',
    'src/withdrawal.rs': 'ac4efb8653653ec919d362e42f4c016206f756db',
    'tests/launch_litesvm.rs': '00e441a282e93d9fcdf49eb7ddd5cff028ff4fd6',
    'tests/submission_window.rs': '30ff95696e9494700bf56f157a64833ea11d5e98',
    'tests/support/e10.rs': '1b86bacb47ca08778d145b889f5ee21e3de0c4b9',
    'tests/support/withdrawal_tests.rs': '49c4e0cb4c73d7261a9cc773e850efe41fe0a54b',
}

def blob_sha(data: bytes) -> str:
    return hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()

def renamed(text: str) -> str:
    for old, new in [(OLD_ID, NEW_ID), ('launch-vault-v6', 'launch-vault-v7'),
                     ('launch_vault_v6', 'launch_vault_v7'), ('launch-v6', 'launch-v7'),
                     ('V6', 'V7'), ('v6-test', 'v7-test'), ('v6-disabled', 'v7-disabled')]:
        text = text.replace(old, new)
    return text

def once(text: str, old: str, new: str) -> str:
    if text.count(old) != 1:
        raise ValueError('PATCH_ANCHOR_NOT_UNIQUE: ' + old[:80])
    return text.replace(old, new, 1)

PREPARE_CONTEXT = '''#[derive(Accounts)]
#[instruction(config: LaunchConfig, spec_hash: [u8; 32], identity: [u8; 32])]
pub struct PreparePolicy<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    /// CHECK: identity only; consent is required at open_prepared_policy.
    pub founder: UncheckedAccount<'info>,
    /// CHECK: identity only; consent is required at open_prepared_policy.
    pub treasury: UncheckedAccount<'info>,
    /// CHECK: identity only; later reports require the oracle signature.
    pub oracle: UncheckedAccount<'info>,
    pub mint: Account<'info, Mint>,
    #[account(init, payer = creator, space = 8 + LaunchPreparationV7::INIT_SPACE,
        seeds = [b"launch-v7-preparation", identity.as_ref()], bump)]
    pub preparation: Box<Account<'info, LaunchPreparationV7>>,
    pub system_program: Program<'info, System>,
}

'''
PREPARATION_FIELD = '''    #[account(seeds = [b"launch-v7-preparation", preparation.identity.as_ref()],
        bump = preparation.bump,
        has_one = creator @ LaunchError::IdentityMismatch,
        has_one = founder @ LaunchError::IdentityMismatch,
        has_one = treasury @ LaunchError::IdentityMismatch,
        has_one = oracle @ LaunchError::IdentityMismatch,
        has_one = mint @ LaunchError::IdentityMismatch,
        constraint = preparation.program == crate::ID @ LaunchError::IdentityMismatch)]
    pub preparation: Box<Account<'info, LaunchPreparationV7>>,
'''
PREPARATION_STATE = '''/// Content-addressed, read-only after creation. No update/close instruction.
/// It carries no token custody, controller, withdrawal or upgrade authority.
#[account]
#[derive(InitSpace)]
pub struct LaunchPreparationV7 {
    pub creator: Pubkey,
    pub mint: Pubkey,
    pub founder: Pubkey,
    pub treasury: Pubkey,
    pub oracle: Pubkey,
    pub program: Pubkey,
    pub identity: [u8; 32],
    pub spec_hash: [u8; 32],
    pub config: LaunchConfig,
    pub bump: u8,
}

'''
MINT_CHECK = '''        require!(
            ctx.accounts.mint.mint_authority.is_none()
                && ctx.accounts.mint.freeze_authority.is_none(),
            LaunchError::MintAuthorityLive
        );
'''
RECOVERY_CHECK = '''        require!(
            config.recovery_keys
                == [
                    ctx.accounts.recovery_one.key(),
                    ctx.accounts.recovery_two.key(),
                    ctx.accounts.recovery_three.key()
                ],
            LaunchError::Unauthorized
        );
'''
PREPARATION_INIT = '''        ctx.accounts.preparation.set_inner(LaunchPreparationV7 {
            creator: ctx.accounts.creator.key(),
            mint: ctx.accounts.mint.key(),
            founder: ctx.accounts.founder.key(),
            treasury: ctx.accounts.treasury.key(),
            oracle: ctx.accounts.oracle.key(),
            program: crate::ID,
            identity,
            spec_hash,
            config,
            bump: ctx.bumps.preparation,
        });
        Ok(())
    }

'''
FIXTURE_OPEN = '''    fn preparation(&self) -> Pubkey {
        Pubkey::find_program_address(&[b"launch-v7-preparation", &self.hash], &ID).0
    }

    fn prepare_ix(&self) -> Instruction {
        ix(accounts::PreparePolicy {
            creator: self.creator.pubkey(), founder: self.founder.pubkey(),
            treasury: self.treasury.pubkey(), oracle: self.oracle.pubkey(), mint: self.mint,
            preparation: self.preparation(), system_program: solana_system_interface::program::ID,
        }, instruction::PreparePolicy { config: self.config, spec_hash: [42; 32], identity: self.hash })
    }

    fn open_ix(&self) -> Instruction {
        ix(accounts::OpenPreparedPolicy {
            creator: self.creator.pubkey(), founder: self.founder.pubkey(),
            treasury: self.treasury.pubkey(), oracle: self.oracle.pubkey(),
            recovery_one: self.recovery[0].pubkey(), recovery_two: self.recovery[1].pubkey(),
            recovery_three: self.recovery[2].pubkey(), mint: self.mint,
            preparation: self.preparation(), policy: self.policy,
            system_program: solana_system_interface::program::ID,
        }, instruction::OpenPreparedPolicy {})
    }

'''
IDENTITY_TEST = '''#[test]
fn identity_prevents_foreign_creator_and_changed_t0_squatting() {
    let mut f = Fixture::new(false, false);
    f.run(f.prepare_ix()).unwrap();
    let prepared = f.svm.get_account(&f.preparation()).unwrap().data;
    let mut stolen = f.open_ix();
    stolen.accounts[0].pubkey = f.outsider.pubkey();
    f.reject_unchanged(stolen, "IdentityMismatch");
    assert_eq!(f.svm.get_account(&f.preparation()).unwrap().data, prepared);
    let mut g = Fixture::new(false, false);
    g.config.t0 += 1;
    assert!(g.run(g.prepare_ix()).is_err());
    assert!(g.svm.get_account(&g.preparation()).is_none());
    assert!(g.svm.get_account(&g.policy).is_none());
    f.run(f.open_ix()).unwrap();
    assert_eq!(f.p().config.t0, f.config.t0);
}

'''
PROBE_OPEN = '''        let preparation = Pubkey::find_program_address(
            &[b"launch-v7-preparation", &identity], &ID).0;
        let preparing = ix(
            accounts::PreparePolicy {
                creator: keys[0].pubkey(), founder: keys[1].pubkey(),
                treasury: keys[2].pubkey(), oracle: keys[3].pubkey(), mint,
                preparation, system_program: solana_system_interface::program::ID,
            },
            instruction::PreparePolicy { config, spec_hash: [42; 32], identity },
        );
        let opening = ix(
            accounts::OpenPreparedPolicy {
                creator: keys[0].pubkey(), founder: keys[1].pubkey(),
                treasury: keys[2].pubkey(), oracle: keys[3].pubkey(),
                recovery_one: keys[4].pubkey(), recovery_two: keys[5].pubkey(),
                recovery_three: keys[6].pubkey(), mint, preparation, policy,
                system_program: solana_system_interface::program::ID,
            },
            instruction::OpenPreparedPolicy {},
        );
'''

def materialize() -> None:
    if NEW.exists():
        raise ValueError('OUTPUT_EXISTS: refusing to overwrite a candidate')
    sources: dict[str, str] = {}
    for path, expected in INPUTS.items():
        raw = (OLD / path).read_bytes()
        if blob_sha(raw) != expected:
            raise ValueError('FROZEN_INPUT_MISMATCH: ' + path)
        sources[path] = renamed(raw.decode())
    lib = sources['src/lib.rs']
    start = lib.index('    pub fn open_policy(')
    end = lib.index('    pub fn deposit(', start)
    old_open = lib[start:end]
    body_start = old_open.index('        require!(')
    init_start = old_open.index('        ctx.accounts.policy.set_inner(')
    validations = old_open[body_start:init_start]
    prepare_validation = once(validations, RECOVERY_CHECK, '')
    prepare = '''    pub fn prepare_policy(
        ctx: Context<PreparePolicy>, config: LaunchConfig,
        spec_hash: [u8; 32], identity: [u8; 32],
    ) -> Result<()> {
''' + MINT_CHECK + prepare_validation + PREPARATION_INIT
    opened = '''    pub fn open_prepared_policy(ctx: Context<OpenPreparedPolicy>) -> Result<()> {
        let config = ctx.accounts.preparation.config;
        let spec_hash = ctx.accounts.preparation.spec_hash;
        let identity = ctx.accounts.preparation.identity;
''' + MINT_CHECK + old_open[body_start:]
    sources['src/lib.rs'] = lib[:start] + prepare + opened + lib[end:]

    ctx = sources['src/contexts.rs']
    start = ctx.index('#[derive(Accounts)]')
    end = ctx.index('#[derive(Accounts)]', start + 1)
    old_context = ctx[start:end]
    opened_context = once(old_context,
        '#[instruction(config: LaunchConfig, spec_hash: [u8; 32], identity: [u8; 32])]\n', '')
    opened_context = opened_context.replace('OpenPolicy', 'OpenPreparedPolicy')
    opened_context = once(opened_context, '    #[account(init, payer = creator, space = 8 + LaunchPolicyV7::INIT_SPACE,',
        PREPARATION_FIELD + '    #[account(init, payer = creator, space = 8 + LaunchPolicyV7::INIT_SPACE,')
    opened_context = once(opened_context,
        'seeds = [b"launch-v7-policy", identity.as_ref()]',
        'seeds = [b"launch-v7-policy", preparation.identity.as_ref()]')
    sources['src/contexts.rs'] = ctx[:start] + PREPARE_CONTEXT + opened_context + ctx[end:]
    sources['src/state.rs'] = once(sources['src/state.rs'],
        '#[account]\n#[derive(InitSpace)]\npub struct LaunchPolicyV7',
        PREPARATION_STATE + '#[account]\n#[derive(InitSpace)]\npub struct LaunchPolicyV7')

    tests = sources['tests/launch_litesvm.rs']
    a = tests.index('    fn open_ix(&self) -> Instruction {')
    b = tests.index('    fn vault(&self, role:', a)
    tests = tests[:a] + FIXTURE_OPEN + tests[b:]
    tests = tests.replace('instruction::OpenPolicy::DISCRIMINATOR', 'instruction::OpenPreparedPolicy::DISCRIMINATOR')
    tests = once(tests, '        let result = self.run_many(vec![instruction], self.creator.pubkey());',
        '''        // Compatibility fixture: two separately signed transactions, never one oversized transaction.
        if opening && self.svm.get_account(&self.preparation()).is_none() {
            self.run_many(vec![self.prepare_ix()], self.creator.pubkey())?;
        }
        let result = self.run_many(vec![instruction], self.creator.pubkey());''')
    a = tests.index('#[test]\nfn identity_prevents_foreign_creator_and_changed_t0_squatting()')
    b = tests.index('#[test]', a + 8)
    tests = tests[:a] + IDENTITY_TEST + tests[b:]
    tests = once(tests,
        'fn consent_required_and_solo_owner_can_fill_all_roles() {\n    let mut f = Fixture::new(false, false);',
        'fn consent_required_and_solo_owner_can_fill_all_roles() {\n    let mut f = Fixture::new(false, false);\n    // Preparation commits separately; the failed consent must preserve it exactly.\n    f.run(f.prepare_ix()).unwrap();')
    tests = once(tests, '    f.config.recovery_keys.swap(0, 1);',
        '    f.config.recovery_keys.swap(0, 1);\n    f.rebind(); // New immutable preparation; old actor order cannot consent to it.')
    extra = ROOT / 'tests/e11b_bootstrap_cases.rs'
    if not extra.is_file() or not extra.stat().st_size:
        raise ValueError('MISSING_BOOTSTRAP_TESTS')
    tests += '\n' + extra.read_text()
    sources['tests/launch_litesvm.rs'] = tests
    sources['tests/support/e10.rs'] = sources['tests/support/e10.rs'].replace('[88; 32]', '[89; 32]')

    probe = sources['tests/submission_window.rs']
    a = probe.index('        let opening = ix(')
    b = probe.index('        let mut p = Self {', a)
    probe = probe[:a] + PROBE_OPEN + probe[b:]
    probe = once(probe, '        p.time(NOW);\n        let tx = p.sign(opening);',
        '        p.time(NOW);\n        let tx = p.sign(preparing);\n        p.svm.send_transaction(tx).unwrap();\n        let tx = p.sign(opening);')
    sources['tests/submission_window.rs'] = probe
    for path, text in sources.items():
        dst = NEW / path
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_text(text)
    manifest = {p: hashlib.sha256((NEW / p).read_bytes()).hexdigest() for p in sorted(sources)}
    receipt = {'schema': 'K4V-E11B-GENERATED-SOURCE-v1', 'status': 'SOURCE_ONLY_NOT_ACCEPTANCE',
               'program_id': NEW_ID, 'test_key_public_insecure': True,
               'frozen_input_blobs': INPUTS, 'generated_sha256': manifest,
               'v6_modified': False, 'production_ready': False}
    out = ROOT / 'target/e11b'
    out.mkdir(parents=True, exist_ok=True)
    (out / 'generated-source.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))

if __name__ == '__main__':
    materialize()
