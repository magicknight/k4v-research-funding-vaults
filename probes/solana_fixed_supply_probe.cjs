// SPDX-License-Identifier: MIT OR Apache-2.0
/*
 * K4V SPL fixed-supply probe. Test environments only.
 *
 * Dependencies used for the 2026-08-09 evidence run:
 *   @solana/web3.js@1
 *   @solana/spl-token@0.4.14
 *
 * SOLANA_CLUSTER may be devnet, testnet or localnet. No key is loaded,
 * persisted or printed: all payer, mint and owner keypairs are ephemeral.
 */
const {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  clusterApiUrl,
} = require('@solana/web3.js');
const {
  AuthorityType,
  createMint,
  getAccount,
  getMint,
  getOrCreateAssociatedTokenAccount,
  mintTo,
  setAuthority,
} = require('@solana/spl-token');

const DECIMALS = 6;
const SCALE = 10n ** BigInt(DECIMALS);
const ALLOCATIONS = {
  founder: 300_000_000n,
  treasury: 500_000_000n,
  genesis: 120_000_000n,
  lp: 80_000_000n,
};
const EXPECTED_SUPPLY = 1_000_000_000n * SCALE;

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function fundEphemeralPayer(connection, payer) {
  const attempts = [1, 0.5, 0.25];
  const errors = [];
  for (let i = 0; i < attempts.length; i += 1) {
    try {
      const signature = await connection.requestAirdrop(
        payer.publicKey,
        Math.floor(attempts[i] * LAMPORTS_PER_SOL),
      );
      await connection.confirmTransaction(signature, 'confirmed');
      const balance = await connection.getBalance(payer.publicKey, 'confirmed');
      if (balance > 0) return { signature, balance };
    } catch (error) {
      errors.push(String(error && error.message ? error.message : error));
      await delay((i + 1) * 2000);
    }
  }
  throw new Error(`test-network airdrop failed: ${errors.join(' | ')}`);
}

async function main() {
  const cluster = process.env.SOLANA_CLUSTER || 'devnet';
  if (!['devnet', 'testnet', 'localnet'].includes(cluster)) {
    throw new Error(`unsupported test cluster: ${cluster}`);
  }
  const rpcEndpoint = cluster === 'localnet'
    ? 'http://127.0.0.1:8899'
    : clusterApiUrl(cluster);
  const connection = new Connection(rpcEndpoint, 'confirmed');
  const payer = Keypair.generate();
  const mintKeypair = Keypair.generate();
  const owners = Object.fromEntries(
    Object.keys(ALLOCATIONS).map((role) => [role, Keypair.generate()]),
  );

  const funding = await fundEphemeralPayer(connection, payer);
  const mint = await createMint(
    connection,
    payer,
    payer.publicKey,
    payer.publicKey,
    DECIMALS,
    mintKeypair,
  );

  const accounts = {};
  const mintTransactions = {};
  for (const [role, wholeAmount] of Object.entries(ALLOCATIONS)) {
    const tokenAccount = await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      mint,
      owners[role].publicKey,
    );
    accounts[role] = tokenAccount.address;
    mintTransactions[role] = await mintTo(
      connection,
      payer,
      mint,
      tokenAccount.address,
      payer,
      wholeAmount * SCALE,
    );
  }

  const revokeMintTx = await setAuthority(
    connection,
    payer,
    mint,
    payer,
    AuthorityType.MintTokens,
    null,
  );
  const revokeFreezeTx = await setAuthority(
    connection,
    payer,
    mint,
    payer,
    AuthorityType.FreezeAccount,
    null,
  );

  const mintState = await getMint(connection, mint, 'confirmed');
  const observed = {};
  for (const [role, address] of Object.entries(accounts)) {
    const state = await getAccount(connection, address, 'confirmed');
    observed[role] = state.amount;
  }

  const allocationChecks = Object.fromEntries(
    Object.entries(ALLOCATIONS).map(([role, wholeAmount]) => [
      role,
      observed[role] === wholeAmount * SCALE,
    ]),
  );
  const checks = {
    cluster_is_non_mainnet_test_environment:
      connection.rpcEndpoint.includes('devnet') ||
      connection.rpcEndpoint.includes('testnet') ||
      connection.rpcEndpoint.includes('127.0.0.1'),
    supply_is_exact: mintState.supply === EXPECTED_SUPPLY,
    allocations_are_exact: Object.values(allocationChecks).every(Boolean),
    mint_authority_revoked: mintState.mintAuthority === null,
    freeze_authority_revoked: mintState.freezeAuthority === null,
  };

  const result = {
    evidence_version: 'PM020-SOL-BASE-v0.1',
    checked_at: new Date().toISOString(),
    network: `solana-${cluster}`,
    rpc_endpoint: connection.rpcEndpoint,
    ephemeral_keys: 'generated in memory; no secret key persisted or emitted',
    mint: mint.toBase58(),
    decimals: DECIMALS,
    expected_supply_raw: EXPECTED_SUPPLY.toString(),
    observed_supply_raw: mintState.supply.toString(),
    allocations: Object.fromEntries(
      Object.entries(ALLOCATIONS).map(([role, wholeAmount]) => [role, {
        whole_tokens: wholeAmount.toString(),
        raw_amount: observed[role].toString(),
        token_account: accounts[role].toBase58(),
        exact: allocationChecks[role],
      }]),
    ),
    transactions: {
      funding_airdrop: funding.signature,
      mint_to: mintTransactions,
      revoke_mint_authority: revokeMintTx,
      revoke_freeze_authority: revokeFreezeTx,
    },
    authorities_after: {
      mint_authority: mintState.mintAuthority,
      freeze_authority: mintState.freezeAuthority,
    },
    checks,
    overall_pass: Object.values(checks).every(Boolean),
    scope_limit: 'Proves SPL fixed supply, exact four-account allocation, and authority revocation only. It does not prove founder/treasury vault locks, IRB/oracle rules, LP behavior, issuer/legal eligibility, or mainnet readiness.',
  };
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  if (!result.overall_pass) process.exitCode = 1;
}

main().catch((error) => {
  process.stderr.write(`${String(error && error.stack ? error.stack : error)}\n`);
  process.exitCode = 1;
});
