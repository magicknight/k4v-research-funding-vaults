// Actual Agave continuation from EXPLICIT PRELOADED application state.
// No public RPC, production keys, account patching after genesis or Clock override.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, createWriteStream, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { finished } from 'node:stream/promises';
import { createHash } from 'node:crypto';
import { Keypair, PublicKey, TransactionInstruction } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { LocalRpc, PROGRAM, LOADER, CLOCK, CODE_SHA256, CODE_BYTES, decodeAccount, pda, integer,
  hash, sleep, readClock, readClockBoundAccounts, signInstructions, submitSigned } from '../clients/launch_v7_local_client.mjs';
import { instruction, UNIT } from './e11b_fixtures.mjs';
import { trackLocalChild, stopLocalChild } from './e11_process.mjs';

const phase = process.argv[2];
assert(['recovery', 'expiry'].includes(phase), 'explicit phase required');
const out = resolve('target/e11c', phase);
const spec = JSON.parse(readFileSync(join(out, 'fixture.json')));
assert.equal(spec.phase, phase); assert.equal(spec.application_state_preloaded, true);
assert.equal(spec.natural_90_180_day_soak, false);
const initial = JSON.parse(readFileSync(join(out, 'preload-snapshot.json')));
const pk = value => new PublicKey(value);
const key = label => Keypair.fromSeed(createHash('sha256').update(label).digest());
const oldFounder = key('e11c-old-founder-public-test-key');
const oldTreasury = key('e11c-old-treasury-public-test-key');
const founder = key('e11b-founder-successor-symbolic-test-key');
const treasury = key('e11b-treasury-successor-symbolic-test-key');
const oracle = key('e11c-oracle-public-test-key');
assert.equal(oldFounder.publicKey.toBase58(), spec.expected.founder);
assert.equal(oldTreasury.publicKey.toBase58(), spec.expected.treasury);
assert.equal(oracle.publicKey.toBase58(), spec.expected.initial_oracle);
const feePayer = Keypair.generate(), loaderAuthority = Keypair.generate();
const policy = pk(spec.expected.policy), mint = pk(spec.expected.mint);
const addr = name => pk(initial.accounts[name].address);
const keyRecord = who => pda(Buffer.from('launch-v7-key'), policy.toBuffer(), who.toBuffer());
const ledger = mkdtempSync(join(tmpdir(), 'k4v-e11c-'));
const log = createWriteStream(join(out, 'validator.log'));
const args = ['--reset','--quiet','--ledger',ledger,'--rpc-port','19599','--faucet-port','19699',
  '--bind-address','127.0.0.1','--dynamic-port-range','19700-19800','--upgradeable-program',
  PROGRAM.toBase58(),resolve('target/v7-test/launch_vault_v7.so'),loaderAuthority.publicKey.toBase58()];
for (const a of spec.accounts) {
  assert(![PROGRAM.toBase58(),CLOCK.toBase58(),LOADER.toBase58()].includes(a.address));
  args.push('--account',a.address,resolve(a.file));
}
const validator = spawn('solana-test-validator',args,{stdio:['ignore','pipe','pipe']});
const tracked = trackLocalChild(validator);
validator.stdout.pipe(log,{end:false});validator.stderr.pipe(log,{end:false});
const rpc = new LocalRpc('http://127.0.0.1:19599');
const receipts = [], refusals = [], observations = [];
const save = (name,value) => writeFileSync(join(out,name+'.json'),JSON.stringify(value,(_,v)=>typeof v==='bigint'?v.toString():v,2)+'\n');
async function until(check, label) {
  const start=Date.now();
  while(Date.now()-start<90000) {
    if(tracked.error||validator.exitCode!==null||validator.signalCode!==null) throw tracked.error??new Error('VALIDATOR_EXIT');
    if(await check()) return;
    await sleep(500);
  }
  throw new Error('TIMEOUT_'+label);
}
async function airdrop(k) {
  const signature=await rpc.call('requestAirdrop',[k.publicKey.toBase58(),1000000000]);
  await until(async()=>{
    const r=await rpc.call('getSignatureStatuses',[[signature]]);
    assert.equal(r.value[0]?.err??null,null);
    return r.value[0]?.confirmationStatus==='finalized';
  },'airdrop');
}
async function send(label,ixs,actors=[]) {
  const envelope=await signInstructions(rpc,ixs,feePayer.publicKey,[feePayer,...actors]);
  const result=await submitSigned(rpc,envelope,envelope.messageHash);
  assert.equal(result.status,'FINALIZED',label+':'+JSON.stringify(result));
  receipts.push({label,...envelope,result});
  console.log('E11C_FINALIZED '+phase+' '+label+' slot='+result.slot);
}
async function reject(label,ixs,actors=[],reason=null) {
  const envelope=await signInstructions(rpc,ixs,feePayer.publicKey,[feePayer,...actors]);
  const result=await submitSigned(rpc,envelope,envelope.messageHash);
  assert.equal(result.status,'SIMULATION_REJECTED',label+':'+JSON.stringify(result));
  if(reason) assert(result.logs.some(s=>s.includes(reason)),label+':'+JSON.stringify(result));
  refusals.push({label,result});
}
function withdrawal(role,nonce,successor,expire=false) {
  return instruction(expire?'expire_withdrawal':'execute_withdrawal',{
    policy,proposal:addr('withdrawal_'+role+'_'+nonce),successor_record:keyRecord(successor)});
}
function release(role,amount,actor,epoch=1n,wrongDestination=false) {
  const name=role===0?'founder':'treasury';
  const values={authority:actor.publicKey,policy,vault:addr(name+'_vault'),mint,
    vault_token:addr(name+'_token'),destination:addr(role===0?(wrongDestination?'founder_destination_0':'founder_destination_1'):'treasury_destination'),
    token_program:TOKEN_PROGRAM_ID};
  if(role===1){values.approval=addr('approval_'+spec.period);values.recipient_record=keyRecord(pk(spec.recipient_owner));}
  return instruction('release',values,[integer(amount),integer(epoch)]);
}
try {
  await until(async()=>{try{return await rpc.call('getHealth')==='ok';}catch{return false;}},'health');
  const genesisHash=await rpc.call('getGenesisHash'),version=await rpc.call('getVersion');
  await airdrop(feePayer);
  const programData=PublicKey.findProgramAddressSync([PROGRAM.toBuffer()],LOADER)[0];
  async function checkProgram(label,authority) {
    const r=await rpc.call('getMultipleAccounts',[[programData.toBase58()],{encoding:'base64',commitment:'finalized'}]);
    const b=decodeAccount(r.value[0],LOADER.toBase58(),false,45+CODE_BYTES);
    assert.equal(b.readUInt32LE(0),3);assert.equal(hash(b.subarray(45)),CODE_SHA256);
    assert.equal(b[12],authority===null?0:1);
    if(authority) assert(b.subarray(13,45).equals(authority.toBuffer()));
    save('program-'+label,{sha256:hash(b.subarray(45)),authority:authority?.toBase58()??null,slot:r.context.slot});
  }
  await checkProgram('genesis',loaderAuthority.publicKey);
  await send('seal-frozen-program',[new TransactionInstruction({programId:LOADER,data:Buffer.from([4,0,0,0]),
    keys:[{pubkey:programData,isSigner:false,isWritable:true},{pubkey:loaderAuthority.publicKey,isSigner:true,isWritable:false}]})],[loaderAuthority]);
  await checkProgram('sealed',null);
  const manifest={schema:'K4V-V7-RPC-REVIEW-MANIFEST-v1',expected:{...spec.expected,genesis_hash:genesisHash},
    external_accounts:spec.external_accounts,approval_periods:spec.approval_periods};
  save('manifest',manifest);
  async function observe(label) {
    const path=join(out,'observation-'+label+'.json');let result;
    for(let attempt=0;attempt<3;attempt++) {
      result=spawnSync('python3',['src/launch_v7_rpc_exporter.py','--rpc-url',rpc.endpoint,'--manifest',join(out,'manifest.json'),'--output',path],
        {encoding:'utf8',env:{...process.env,PYTHONPATH:'src'}});
      if(result.status===0) break;
      let e;try{e=JSON.parse(result.stdout);}catch{}
      if(e?.error!=='RPC_CLOCK_BANK_MISMATCH') break;
      await sleep(200);
    }
    assert.equal(result.status,0,result.stdout+result.stderr);
    const observation=JSON.parse(readFileSync(path));assert.equal(observation.verification.valid,true);
    observations.push({label,slot:observation.snapshot.slot,now:observation.snapshot.now,sha256:hash(readFileSync(path))});
    return observation;
  }
  // Every imported application byte must still match before any application transaction.
  const {response}=await readClockBoundAccounts(rpc,[...spec.accounts.map(a=>a.address),CLOCK.toBase58()]);
  spec.accounts.forEach((a,i)=>{
    const actual=response.value[i];assert(actual);assert.equal(actual.owner,a.owner);assert.equal(actual.executable,a.executable);
    assert.equal(hash(Buffer.from(actual.data[0],'base64')),a.data_sha256,'PRELOAD_BYTES_'+a.name);
  });
  const now=(await readClock(rpc)).now,t0=BigInt(spec.t0),period=2592000n;
  assert.equal((now-t0)/period,BigInt(spec.period),'FIXTURE_OUTSIDE_REQUIRED_PERIOD');
  assert(now>=t0+BigInt(spec.period)*period+60n,'FIXTURE_NOT_MATURE');
  await observe('preloaded');
  if(phase==='recovery') {
    await reject('founder-paused-before-recovery',[release(0,UNIT,oldFounder,0n,true)],[oldFounder],'WithdrawalPaused');
    await reject('treasury-paused-before-recovery',[release(1,UNIT,oldTreasury,0n)],[oldTreasury],'WithdrawalPaused');
    await send('execute-founder-recovery',[withdrawal(0,1,founder.publicKey)]);
    await send('execute-treasury-recovery',[withdrawal(1,1,treasury.publicKey)]);
  } else {
    await reject('expired-proposal-cannot-execute',[withdrawal(0,3,pk(spec.depositor))],[],'WithdrawalWindow');
    await send('expire-founder-proposal',[withdrawal(0,3,pk(spec.depositor),true)]);
  }
  await observe('authority-action');
  const reportAt=(await readClock(rpc)).now;
  await send('refresh-capacity',[instruction('report_capacity',{oracle:oracle.publicKey,policy},
    [integer(BigInt(spec.capacity)),integer(reportAt,true),integer(BigInt(spec.report_sequence)+1n),integer(BigInt(spec.oracle_epoch))])],[oracle]);
  await send('founder-continues-withdrawing',[release(0,100000n*UNIT,founder)],[founder]);
  await send('treasury-uses-existing-budget',[release(1,150000n*UNIT,treasury)],[treasury]);
  await observe('continued');
  await reject('old-founder-retired',[release(0,UNIT,oldFounder,0n,true)],[oldFounder],'WithdrawalAuthority');
  await reject('old-treasury-retired',[release(1,UNIT,oldTreasury,0n)],[oldTreasury],'WithdrawalAuthority');
  await reject('stale-epoch',[release(0,UNIT,founder,0n)],[founder],'WithdrawalAuthority');
  await reject('wrong-destination',[release(0,UNIT,founder,1n,true)],[founder],'Unauthorized');
  await reject('founder-quota-plus-one',[release(0,900000n*UNIT+1n,founder)],[founder],'ReservedQuotaExceeded');
  await reject('treasury-budget-plus-one',[release(1,150000n*UNIT+1n,treasury)],[treasury],'InvalidApproval');
  await reject('proposal-replay',[phase==='recovery'?withdrawal(0,1,founder.publicKey):withdrawal(0,3,pk(spec.depositor),true)]);
  await observe('after-refusals');
  assert.equal(await rpc.call('getGenesisHash'),genesisHash);
  const check=spawnSync('python3',['tools/verify_e11c_continuation.py',out],{encoding:'utf8'});
  assert.equal(check.status,0,check.stdout+check.stderr);
  const verified=JSON.parse(check.stdout);assert.equal(verified.valid,true);save('continuity',verified);
  const receipt={schema:'K4V-E11C-VALIDATOR-CONTINUATION-v1',valid:true,phase,
    scope:'ACTUAL_LOCAL_AGAVE_FROM_EXPLICIT_PRELOADED_APPLICATION_STATE',version,genesis_hash:genesisHash,
    program:PROGRAM.toBase58(),program_sha256:CODE_SHA256,program_source_modified:false,
    application_state_preloaded:true,fixture_clock_controlled:true,actual_validator_clock_override:false,
    natural_90_180_day_soak:false,separate_validator_fixture:true,public_chain_transactions:0,
    private_keys_serialized:false,independent_human_governance:false,production_ready:false,
    finalized_client_transactions:receipts.length,refusals,raw_checkpoints:observations,continuity:verified};
  save('receipt',receipt);console.log('E11C_CONTINUATION_PASS '+phase+' finalized='+receipts.length);
} catch(error) {
  save('failure',{message:error.message,stack:error.stack,finalized:receipts.length});throw error;
} finally {
  save('signed-transactions',receipts);
  try{await stopLocalChild(validator,tracked);}finally{log.end();await finished(log);rmSync(ledger,{recursive:true,force:true});}
}
