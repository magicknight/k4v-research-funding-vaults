// SPDX-License-Identifier: MIT OR Apache-2.0
//
// R3-B2: a full-scale local one-mint rehearsal whose B2 policy and purpose
// authority are a real Squads v4 2-of-3 vault PDA. The probe replaces one
// member, approves with the post-replacement key set, time-travels on Surfpool,
// and executes a release without changing the address B2 stores.
//
// Localnet only. Every key is process-local and is never serialized.

import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  TransactionMessage,
} from "@solana/web3.js";
import {
  AuthorityType,
  TOKEN_PROGRAM_ID,
  createAccount,
  createMint,
  getAccount,
  getMint,
  mintTo,
  setAuthority,
} from "@solana/spl-token";
import * as multisig from "@sqds/multisig";
import { createHash, randomBytes } from "crypto";
import fs from "fs";

const RPC = process.env.K4V_SURFPOOL_RPC ?? "http://127.0.0.1:19199";
if (!RPC.startsWith("http://127.0.0.1:") && !RPC.startsWith("http://localhost:")) {
  throw new Error("R3 full-scale Squads probe is restricted to loopback RPC");
}
const connection = new Connection(RPC, "confirmed");
const B2 = new PublicKey("2FZ5XPBDQhdsbpj7VnFVZ5agFkMYFgEXMchAZyRWe65w");
const SQUADS = new PublicKey("SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf");
const SQUADS_CONFIG = new PublicKey("BSTq9w3kZwNwpBXJEvTZz2G9ZTNyKBvoSeXMvwb4cNZr");
const SQUADS_TREASURY = new PublicKey("HM5y4mz3Bt9JY9mr1hkyhnvqxSH4H2u2451j7Hc2dtvK");
const B2_SBF = new URL("../target/deploy/purpose_vault.so", import.meta.url);
const CANDIDATE_CONFIG = process.env.K4V_R3_CANDIDATE_CONFIG;
if (!CANDIDATE_CONFIG) throw new Error("K4V_R3_CANDIDATE_CONFIG must name the explicit open candidate");

const DECIMALS = 9;
const FOUNDER = 300_000_000_000_000_000n;
const TREASURY = 500_000_000_000_000_000n;
const GENESIS = 120_000_000_000_000_000n;
const LP = 80_000_000_000_000_000n;
const SUPPLY = FOUNDER + TREASURY + GENESIS + LP;
const ANNUAL_BPS = 500;
const FOUNDER_CAP = 1_250_000_000_000_000n;
const TREASURY_CAP = 2_083_333_333_333_333n;
const ELIGIBLE_VOLUME = 120_000_000_000_000_000n;
const MARKET_CAPACITY = 3_000_000_000_000_000n;
const MARKET_BPS = 250;
const MAX_AGE = 3 * 24 * 3600;
const CLIFF = 730 * 24 * 3600;
const PERIOD = 30 * 24 * 3600;
const U64_MAX = 18_446_744_073_709_551_615n;
const POST_REPLACEMENT_PERIOD = BigInt(Math.floor(CLIFF / PERIOD) + 2);
const POLICY_HASH = randomBytes(32);

const record = {
  schema: "k4v-r3-b2-full-scale-squads-local/v0.1",
  epistemic_status: "LOCAL_TEST_EVIDENCE_NOT_INDEPENDENT",
  cluster: "local-surfpool-devnet-fork",
  rpc: RPC,
  no_official_mint: true,
  mainnet_authorized: false,
  production_keys_used: false,
  private_keys_serialized: false,
  transactions: {},
  checks: {},
};
const say = (...args) => console.log(args.map(String).join(" "));
const disc = name => createHash("sha256").update("global:" + name).digest().subarray(0, 8);
const u16 = value => { const b = Buffer.alloc(2); b.writeUInt16LE(Number(value)); return b; };
const u64 = value => {
  if (typeof value === "number" && !Number.isSafeInteger(value)) {
    throw new TypeError("unsafe JavaScript number rejected before u64 encoding");
  }
  const integer = BigInt(value);
  if (integer < 0n || integer > U64_MAX) throw new RangeError("u64 amount out of range");
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(integer);
  return b;
};
const i64 = value => { const b = Buffer.alloc(8); b.writeBigInt64LE(BigInt(value)); return b; };

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function countOpen(value) {
  if (Array.isArray(value)) return value.reduce((sum, item) => sum + countOpen(item), 0);
  if (value && typeof value === "object") {
    return (value.state === "OPEN" ? 1 : 0) +
      Object.values(value).reduce((sum, item) => sum + countOpen(item), 0);
  }
  return 0;
}

async function rpc(method, params) {
  const response = await connection._rpcRequest(method, params);
  if (response.error) throw new Error(`${method}: ${JSON.stringify(response.error)}`);
  return response.result;
}

async function confirm(signature) {
  const result = await connection.confirmTransaction(signature, "confirmed");
  if (result.value.err) throw new Error(`transaction ${signature} failed: ${JSON.stringify(result.value.err)}`);
  return signature;
}

async function fund(pubkey, sol = 20) {
  return confirm(await connection.requestAirdrop(pubkey, sol * LAMPORTS_PER_SOL));
}

async function createProposal(multisigPda, transactionIndex, creator, payer) {
  const instruction = multisig.instructions.proposalCreate({
    multisigPda,
    creator: creator.publicKey,
    rentPayer: payer.publicKey,
    transactionIndex,
  });
  const tx = new Transaction().add(instruction);
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
  tx.sign(payer, creator);
  const simulation = await connection.simulateTransaction(tx);
  if (simulation.value.err) {
    throw new Error(`proposalCreate simulation failed: ${JSON.stringify(simulation.value.err)}\n${(simulation.value.logs ?? []).join("\n")}`);
  }
  return confirm(await connection.sendRawTransaction(tx.serialize(), { skipPreflight: false }));
}

async function direct(instruction, payer, extraSigners, label) {
  const tx = new Transaction().add(instruction);
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await connection.getLatestBlockhash()).blockhash;
  tx.sign(payer, ...extraSigners);
  const simulation = await connection.simulateTransaction(tx);
  if (simulation.value.err) {
    throw new Error(`${label} simulation failed: ${JSON.stringify(simulation.value.err)}\n${(simulation.value.logs ?? []).join("\n")}`);
  }
  const signature = await confirm(await connection.sendRawTransaction(tx.serialize(), { skipPreflight: false }));
  record.transactions[label] = signature;
  say(label, signature);
  return signature;
}

async function throughMultisig({
  instructions,
  label,
  multisigPda,
  vaultPda,
  payer,
  voters,
  extraSigners = [],
}) {
  const info = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const transactionIndex = BigInt(info.transactionIndex) + 1n;
  const message = new TransactionMessage({
    payerKey: vaultPda,
    recentBlockhash: (await connection.getLatestBlockhash()).blockhash,
    instructions,
  });
  await confirm(await multisig.rpc.vaultTransactionCreate({
    connection,
    feePayer: payer,
    multisigPda,
    transactionIndex,
    creator: voters[0].publicKey,
    rentPayer: payer.publicKey,
    vaultIndex: 0,
    ephemeralSigners: 0,
    transactionMessage: message,
    signers: [payer, voters[0]],
    sendOptions: { skipPreflight: false },
  }));
  await createProposal(multisigPda, transactionIndex, voters[0], payer);
  for (const voter of voters) {
    await confirm(await multisig.rpc.proposalApprove({
      connection,
      feePayer: payer,
      multisigPda,
      transactionIndex,
      member: voter,
      sendOptions: { skipPreflight: false },
    }));
  }
  const signature = await confirm(await multisig.rpc.vaultTransactionExecute({
    connection,
    feePayer: payer,
    multisigPda,
    transactionIndex,
    member: voters[0].publicKey,
    signers: [payer, voters[0], ...extraSigners],
    sendOptions: { skipPreflight: false },
  }));
  record.transactions[label] = signature;
  say(label, signature);
  return signature;
}

function vaultPdas(kind, authority, mint) {
  const [vault] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-vault"), POLICY_HASH, Buffer.from([kind]), authority.toBuffer(), mint.toBuffer()],
    B2,
  );
  const [token] = PublicKey.findProgramAddressSync([Buffer.from("purpose-token"), vault.toBuffer()], B2);
  return [vault, token];
}

function approvalPda(vault, period) {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-approval"), vault.toBuffer(), u64(period)],
    B2,
  )[0];
}

function decodePolicy(data) {
  let offset = 8;
  const pk = () => { const value = new PublicKey(data.subarray(offset, offset + 32)); offset += 32; return value; };
  const b32 = () => { const value = data.subarray(offset, offset + 32); offset += 32; return value; };
  const i = () => { const value = data.readBigInt64LE(offset); offset += 8; return value; };
  const u = () => { const value = data.readBigUInt64LE(offset); offset += 8; return value; };
  const u32 = () => { const value = data.readUInt32LE(offset); offset += 4; return value; };
  return {
    authority: pk(), mint: pk(), policyHash: b32(), genesisTs: i(), currentPeriod: u(),
    releasedThisPeriod: u(), hardCeiling: u(), silenceFloor: u(), silenceGrace: i(),
    vaultCount: u32(), bump: data[offset],
  };
}

function decodeVault(data) {
  let offset = 8;
  const u8 = () => data[offset++];
  const pk = () => { const value = new PublicKey(data.subarray(offset, offset + 32)); offset += 32; return value; };
  const b32 = () => { const value = data.subarray(offset, offset + 32); offset += 32; return value; };
  const u = () => { const value = data.readBigUInt64LE(offset); offset += 8; return value; };
  const i = () => { const value = data.readBigInt64LE(offset); offset += 8; return value; };
  const u16v = () => { const value = data.readUInt16LE(offset); offset += 2; return value; };
  return {
    kind: u8(), depositor: pk(), authority: pk(), mint: pk(), policyHash: b32(), deposited: u(),
    monthlyCap: u(), releasedTotal: u(), releasedThisPeriod: u(), currentPeriod: u(),
    genesisTs: i(), cliffEndTs: i(), annualBps: u16v(), decimals: u8(),
  };
}

async function main() {
  assert(SUPPLY === 1_000_000_000_000_000_000n, "full supply arithmetic drifted");
  let unsafeNumberRejected = false;
  try { u64(Number(FOUNDER)); } catch (error) {
    unsafeNumberRejected = error instanceof TypeError;
  }
  assert(unsafeNumberRejected, "R3-N12 unsafe JavaScript number guard did not fire");
  const candidateBytes = fs.readFileSync(CANDIDATE_CONFIG);
  const candidate = JSON.parse(candidateBytes);
  const openParameterCount = candidate.open_parameter_count ?? countOpen(candidate);
  assert(candidate.mainnet_authorized === false && candidate.official_mint === null,
    "candidate lost its explicit no-mainnet boundary");
  assert(openParameterCount === 26, `expected 26 OPEN candidate values, got ${openParameterCount}`);
  assert(await connection.getAccountInfo(SQUADS), "Squads program was not cloned from devnet");
  assert(await connection.getAccountInfo(SQUADS_CONFIG), "Squads program config was not cloned from devnet");
  assert(await connection.getAccountInfo(SQUADS_TREASURY), "Squads treasury was not cloned from devnet");

  const sbf = fs.readFileSync(B2_SBF);
  record.candidate_config = {
    sha256: createHash("sha256").update(candidateBytes).digest("hex"),
    open_parameter_count: openParameterCount,
    test_values_do_not_freeze_production: true,
  };
  record.program = {
    id: B2.toBase58(),
    sbf_sha256: createHash("sha256").update(sbf).digest("hex"),
    sbf_bytes: sbf.length,
  };
  record.squads_program = SQUADS.toBase58();
  await rpc("surfnet_writeProgram", [B2.toBase58(), sbf.toString("hex"), 0]);
  assert((await connection.getAccountInfo(B2))?.executable, "B2 SBF was not loaded into Surfpool");

  const payer = Keypair.generate();
  const depositor = Keypair.generate();
  const beneficiary = Keypair.generate();
  const oracle = Keypair.generate();
  const contractor = Keypair.generate();
  const members = [Keypair.generate(), Keypair.generate(), Keypair.generate()];
  for (const key of [payer, depositor, beneficiary, oracle, contractor, ...members]) {
    await fund(key.publicKey);
  }

  const createKey = Keypair.generate();
  const [multisigPda] = multisig.getMultisigPda({ createKey: createKey.publicKey });
  const [vaultPda] = multisig.getVaultPda({ multisigPda, index: 0 });
  await fund(vaultPda);
  await confirm(await multisig.rpc.multisigCreateV2({
    connection,
    treasury: SQUADS_TREASURY,
    createKey,
    creator: payer,
    multisigPda,
    configAuthority: null,
    threshold: 2,
    timeLock: 0,
    rentCollector: null,
    members: members.map(member => ({ key: member.publicKey, permissions: multisig.types.Permissions.all() })),
    sendOptions: { skipPreflight: false },
  }));
  record.multisig = multisigPda.toBase58();
  record.vault_pda = vaultPda.toBase58();

  const mintKeypair = Keypair.generate();
  const mint = await createMint(connection, payer, payer.publicKey, payer.publicKey, DECIMALS, mintKeypair);
  const founderToken = await createAccount(
    connection, payer, mint, depositor.publicKey, Keypair.generate(),
  );
  const treasuryToken = await createAccount(
    connection, payer, mint, depositor.publicKey, Keypair.generate(),
  );
  const genesisToken = await createAccount(
    connection, payer, mint, Keypair.generate().publicKey, Keypair.generate(),
  );
  const lpToken = await createAccount(
    connection, payer, mint, Keypair.generate().publicKey, Keypair.generate(),
  );
  const beneficiaryToken = await createAccount(
    connection, payer, mint, beneficiary.publicKey, Keypair.generate(),
  );
  const contractorToken = await createAccount(
    connection, payer, mint, contractor.publicKey, Keypair.generate(),
  );
  record.transactions.mint_founder = await mintTo(connection, payer, mint, founderToken, payer, FOUNDER);
  record.transactions.mint_treasury = await mintTo(connection, payer, mint, treasuryToken, payer, TREASURY);
  record.transactions.mint_genesis = await mintTo(connection, payer, mint, genesisToken, payer, GENESIS);
  record.transactions.mint_lp = await mintTo(connection, payer, mint, lpToken, payer, LP);
  record.transactions.revoke_mint = await setAuthority(
    connection, payer, mint, payer, AuthorityType.MintTokens, null,
  );
  record.transactions.revoke_freeze = await setAuthority(
    connection, payer, mint, payer, AuthorityType.FreezeAccount, null,
  );

  const [policyPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-policy"), POLICY_HASH], B2,
  );
  const [marketPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("purpose-market"), POLICY_HASH], B2,
  );
  const context = { multisigPda, vaultPda, payer, voters: [members[0], members[1]] };
  const openPolicy = new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: vaultPda, isSigner: true, isWritable: true },
      { pubkey: oracle.publicKey, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: policyPda, isSigner: false, isWritable: true },
      { pubkey: marketPda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([
      disc("open_policy"), POLICY_HASH, u16(MARKET_BPS), i64(MAX_AGE), u64(U64_MAX), u64(0), i64(0),
    ]),
  });
  await throughMultisig({ ...context, instructions: [openPolicy], label: "open_policy" });

  const report = new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: oracle.publicKey, isSigner: true, isWritable: false },
      { pubkey: marketPda, isSigner: false, isWritable: true },
    ],
    data: Buffer.concat([disc("report_volume"), u64(ELIGIBLE_VOLUME)]),
  });
  await direct(report, payer, [oracle], "report_volume_initial");

  const [beneficiaryVault, beneficiaryVaultToken] = vaultPdas(0, beneficiary.publicKey, mint);
  const [purposeVault, purposeVaultToken] = vaultPdas(1, vaultPda, mint);
  const deposit = (kind, authority, source, vault, vaultToken, amount, cliff) =>
    new TransactionInstruction({
      programId: B2,
      keys: [
        { pubkey: depositor.publicKey, isSigner: true, isWritable: true },
        { pubkey: vaultPda, isSigner: true, isWritable: false },
        { pubkey: authority, isSigner: false, isWritable: false },
        { pubkey: mint, isSigner: false, isWritable: false },
        { pubkey: source, isSigner: false, isWritable: true },
        { pubkey: policyPda, isSigner: false, isWritable: true },
        { pubkey: vault, isSigner: false, isWritable: true },
        { pubkey: vaultToken, isSigner: false, isWritable: true },
        { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      data: Buffer.concat([disc("deposit"), Buffer.from([kind]), u64(amount), u16(ANNUAL_BPS), i64(cliff)]),
    });
  await throughMultisig({
    ...context,
    instructions: [deposit(0, beneficiary.publicKey, founderToken, beneficiaryVault, beneficiaryVaultToken, FOUNDER, CLIFF)],
    extraSigners: [depositor],
    label: "deposit_founder",
  });
  await throughMultisig({
    ...context,
    instructions: [deposit(1, vaultPda, treasuryToken, purposeVault, purposeVaultToken, TREASURY, 0)],
    extraSigners: [depositor],
    label: "deposit_treasury",
  });

  const approveInstruction = (period, pda, need) => new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: vaultPda, isSigner: true, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: purposeVault, isSigner: false, isWritable: false },
      { pubkey: policyPda, isSigner: false, isWritable: false },
      { pubkey: contractorToken, isSigner: false, isWritable: false },
      { pubkey: pda, isSigner: false, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("approve"), u64(period), u64(need)]),
  });
  const beforePeriod = POST_REPLACEMENT_PERIOD - 1n;
  const approvalBefore = approvalPda(purposeVault, beforePeriod);
  await throughMultisig({
    ...context,
    instructions: [approveInstruction(beforePeriod, approvalBefore, 1n)],
    label: "approve_before_replacement",
  });

  const replacement = Keypair.generate();
  await fund(replacement.publicKey);
  const before = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const configIndex = BigInt(before.transactionIndex) + 1n;
  await confirm(await multisig.rpc.configTransactionCreate({
    connection,
    feePayer: payer,
    multisigPda,
    transactionIndex: configIndex,
    creator: members[0].publicKey,
    rentPayer: payer.publicKey,
    actions: [
      { __kind: "RemoveMember", oldMember: members[2].publicKey },
      { __kind: "AddMember", newMember: { key: replacement.publicKey, permissions: multisig.types.Permissions.all() } },
    ],
    signers: [payer, members[0]],
    sendOptions: { skipPreflight: false },
  }));
  await createProposal(multisigPda, configIndex, members[0], payer);
  for (const voter of [members[0], members[1]]) {
    await confirm(await multisig.rpc.proposalApprove({
      connection, feePayer: payer, multisigPda, transactionIndex: configIndex,
      member: voter, sendOptions: { skipPreflight: false },
    }));
  }
  record.transactions.replace_member = await confirm(await multisig.rpc.configTransactionExecute({
    connection,
    feePayer: payer,
    multisigPda,
    transactionIndex: configIndex,
    member: members[0],
    rentPayer: payer,
    signers: [payer, members[0]],
    sendOptions: { skipPreflight: false },
  }));
  const after = await multisig.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const afterKeys = after.members.map(member => member.key.toBase58());

  const approvalAfter = approvalPda(purposeVault, POST_REPLACEMENT_PERIOD);
  const postContext = { ...context, voters: [members[0], replacement] };
  await throughMultisig({
    ...postContext,
    instructions: [approveInstruction(POST_REPLACEMENT_PERIOD, approvalAfter, TREASURY_CAP)],
    label: "approve_after_replacement",
  });

  const policyBeforeTravel = decodePolicy((await connection.getAccountInfo(policyPda)).data);
  const targetTimestamp = policyBeforeTravel.genesisTs + BigInt(PERIOD) * POST_REPLACEMENT_PERIOD;
  await rpc("surfnet_timeTravel", [{ absoluteTimestamp: Number(targetTimestamp * 1000n) }]);
  await direct(report, payer, [oracle], "report_volume_after_time_travel");

  const releaseBeneficiary = new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: beneficiary.publicKey, isSigner: true, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: beneficiaryVault, isSigner: false, isWritable: true },
      { pubkey: policyPda, isSigner: false, isWritable: true },
      { pubkey: marketPda, isSigner: false, isWritable: false },
      { pubkey: beneficiaryVaultToken, isSigner: false, isWritable: true },
      { pubkey: beneficiaryToken, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("release_beneficiary"), u64(FOUNDER_CAP)]),
  });
  await direct(releaseBeneficiary, payer, [beneficiary], "release_founder");

  const purposeAmount = MARKET_CAPACITY - FOUNDER_CAP;
  const releasePurpose = new TransactionInstruction({
    programId: B2,
    keys: [
      { pubkey: vaultPda, isSigner: true, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: purposeVault, isSigner: false, isWritable: true },
      { pubkey: policyPda, isSigner: false, isWritable: true },
      { pubkey: marketPda, isSigner: false, isWritable: false },
      { pubkey: purposeVaultToken, isSigner: false, isWritable: true },
      { pubkey: contractorToken, isSigner: false, isWritable: true },
      { pubkey: approvalAfter, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([disc("release_purpose"), u64(purposeAmount)]),
  });
  await throughMultisig({
    ...postContext,
    instructions: [releasePurpose],
    label: "release_after_replacement",
  });

  const mintState = await getMint(connection, mint);
  const founderVaultState = decodeVault((await connection.getAccountInfo(beneficiaryVault)).data);
  const purposeVaultState = decodeVault((await connection.getAccountInfo(purposeVault)).data);
  const policyState = decodePolicy((await connection.getAccountInfo(policyPda)).data);
  const final = {
    founderStaging: (await getAccount(connection, founderToken)).amount,
    treasuryStaging: (await getAccount(connection, treasuryToken)).amount,
    genesis: (await getAccount(connection, genesisToken)).amount,
    lp: (await getAccount(connection, lpToken)).amount,
    founderVault: (await getAccount(connection, beneficiaryVaultToken)).amount,
    purposeVault: (await getAccount(connection, purposeVaultToken)).amount,
    beneficiary: (await getAccount(connection, beneficiaryToken)).amount,
    contractor: (await getAccount(connection, contractorToken)).amount,
  };
  const conserved = Object.values(final).reduce((sum, value) => sum + value, 0n);
  const sameVault = multisig.getVaultPda({ multisigPda, index: 0 })[0].equals(vaultPda);
  Object.assign(record, {
    mint: mint.toBase58(),
    decimals: DECIMALS,
    supply: mintState.supply.toString(),
    policy: policyPda.toBase58(),
    market: marketPda.toBase58(),
    policy_hash: POLICY_HASH.toString("hex"),
    oracle: oracle.publicKey.toBase58(),
    founder_staging: founderToken.toBase58(),
    treasury_staging: treasuryToken.toBase58(),
    genesis_account: genesisToken.toBase58(),
    lp_account: lpToken.toBase58(),
    beneficiary_vault: beneficiaryVault.toBase58(),
    beneficiary_vault_token: beneficiaryVaultToken.toBase58(),
    purpose_vault: purposeVault.toBase58(),
    purpose_vault_token: purposeVaultToken.toBase58(),
    approval_before_replacement: approvalBefore.toBase58(),
    approval_after_replacement: approvalAfter.toBase58(),
    beneficiary_destination: beneficiaryToken.toBase58(),
    contractor_destination: contractorToken.toBase58(),
    member_replacement: {
      lost: members[2].publicKey.toBase58(),
      added: replacement.publicKey.toBase58(),
      members_after: afterKeys,
      lost_absent: !afterKeys.includes(members[2].publicKey.toBase58()),
      replacement_present: afterKeys.includes(replacement.publicKey.toBase58()),
      multisig_address_unchanged: true,
      vault_pda_unchanged: sameVault,
    },
    balances: Object.fromEntries(Object.entries(final).map(([key, value]) => [key, value.toString()])),
    policy_released_this_period: policyState.releasedThisPeriod.toString(),
    founder_monthly_cap: founderVaultState.monthlyCap.toString(),
    purpose_monthly_cap: purposeVaultState.monthlyCap.toString(),
    conserved_total: conserved.toString(),
  });
  record.checks = {
    unsafe_javascript_number_rejected_before_encoding: unsafeNumberRejected,
    exact_supply: mintState.supply === SUPPLY,
    mint_authority_revoked: mintState.mintAuthority === null,
    freeze_authority_revoked: mintState.freezeAuthority === null,
    both_staging_accounts_empty: final.founderStaging === 0n && final.treasuryStaging === 0n,
    four_pool_and_release_conservation: conserved === SUPPLY,
    full_founder_and_treasury_deposits: founderVaultState.deposited === FOUNDER && purposeVaultState.deposited === TREASURY,
    exact_full_scale_caps: founderVaultState.monthlyCap === FOUNDER_CAP && purposeVaultState.monthlyCap === TREASURY_CAP,
    purpose_authority_is_stable_squads_vault: purposeVaultState.authority.equals(vaultPda) && sameVault,
    member_replaced: !afterKeys.includes(members[2].publicKey.toBase58()) && afterKeys.includes(replacement.publicKey.toBase58()),
    post_replacement_release_consumed_shared_window: policyState.releasedThisPeriod === MARKET_CAPACITY,
    post_replacement_purpose_release_exact: final.contractor === purposeAmount,
  };
  record.valid = Object.values(record.checks).every(Boolean);
  if (!record.valid) throw new Error(`R3-B2 checks failed: ${JSON.stringify(record.checks)}`);
  record.result = "PASS";
  const output = process.env.K4V_R3_SQUADS_RECEIPT_OUT;
  if (output) fs.writeFileSync(output, JSON.stringify(record, null, 2));
  console.log(`RESULT_JSON ${JSON.stringify(record)}`);
}

main().catch(error => {
  record.result = "FAIL";
  record.error = String(error?.stack ?? error);
  const output = process.env.K4V_R3_SQUADS_RECEIPT_OUT;
  if (output) fs.writeFileSync(output, JSON.stringify(record, null, 2));
  console.error(record.error);
  process.exitCode = 1;
});
