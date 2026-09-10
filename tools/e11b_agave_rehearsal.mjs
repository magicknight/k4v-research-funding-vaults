// LOCAL ONLY. Six distinct role keys + a separate fee payer; no public RPC or wallet persistence.
import assert from 'node:assert/strict';
import { readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync,createWriteStream } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve,join } from 'node:path';
import { spawn,spawnSync } from 'node:child_process';
import { finished } from 'node:stream/promises';
import { Keypair,PublicKey,SystemProgram,TransactionInstruction } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID,MINT_SIZE,ACCOUNT_SIZE,AuthorityType,createInitializeMint2Instruction,
  createInitializeAccount3Instruction,createMintToCheckedInstruction,createSetAuthorityInstruction } from '@solana/spl-token';
import { LocalRpc,PROGRAM,LOADER,CLOCK,SYSTEM,CODE_SHA256,pda,integer,hash,sleep,readClock,
  readClockBoundAccounts,signInstructions,submitSigned,readBoundPolicy } from '../clients/launch_v7_local_client.mjs';
import { fixture,instruction,prepareInstruction,openInstruction,preparationAddress,SUPPLY } from './e11b_fixtures.mjs';
import { encodeConfig,boundLaunchIdentity } from '../probes/launch_v7_identity.mjs';
import { trackLocalChild,stopLocalChild } from './e11_process.mjs';

const out=resolve('target/e11b');mkdirSync(out,{recursive:true});
const ledger=mkdtempSync(join(tmpdir(),'k4v-e11b-'));
const log=createWriteStream(join(out,'validator.log'));
const loaderAuthority=Keypair.generate();
const validator=spawn('solana-test-validator',['--reset','--quiet','--ledger',ledger,
  '--rpc-port','19599','--faucet-port','19699','--bind-address','127.0.0.1','--dynamic-port-range','19700-19800',
  '--upgradeable-program',PROGRAM.toBase58(),resolve('target/v7-test/launch_vault_v7.so'),loaderAuthority.publicKey.toBase58()],
  {stdio:['ignore','pipe','pipe']});
const tracked=trackLocalChild(validator);
validator.stdout.pipe(log,{end:false});validator.stderr.pipe(log,{end:false});
const rpc=new LocalRpc('http://127.0.0.1:19599');
const receipts=[],refusals=[];
const save=(name,value)=>writeFileSync(join(out,name+'.json'),JSON.stringify(value,(_,v)=>typeof v==='bigint'?v.toString():v,2)+'\n');
async function until(check,timeout,label){
  const start=Date.now();
  while(Date.now()-start<timeout){
    if(tracked.error||validator.exitCode!==null||validator.signalCode!==null)throw tracked.error??new Error('VALIDATOR_EXIT');
    if(await check())return;
    await sleep(500);
  }
  throw new Error('TIMEOUT_'+label);
}
async function airdrop(key,amount){
  const sig=await rpc.call('requestAirdrop',[key.toBase58(),amount]);
  await until(async()=>{
    const r=await rpc.call('getSignatureStatuses',[[sig]]);
    assert.equal(r.value[0]?.err??null,null);
    return r.value[0]?.confirmationStatus==='finalized';
  },90000,'airdrop');
}
async function send(label,ixs,payer,signers){
  const envelope=await signInstructions(rpc,ixs,payer.publicKey,signers);
  const result=await submitSigned(rpc,envelope,envelope.messageHash);
  assert.equal(result.status,'FINALIZED',label+':'+JSON.stringify(result));
  receipts.push({label,...envelope,result});
  console.log('E11B_FINALIZED '+label+' bytes='+envelope.byteLength+' slot='+result.slot);
  return result;
}
async function reject(label,ixs,payer,signers){
  const envelope=await signInstructions(rpc,ixs,payer.publicKey,signers);
  const result=await submitSigned(rpc,envelope,envelope.messageHash);
  assert.equal(result.status,'SIMULATION_REJECTED',label+':'+JSON.stringify(result));
  refusals.push({label,result});
}
async function bytesOf(address){
  const r=await rpc.call('getMultipleAccounts',[[address.toBase58()],{encoding:'base64',commitment:'finalized'}]);
  assert.equal(r.value[0].owner,PROGRAM.toBase58());assert.equal(r.value[0].executable,false);
  return Buffer.from(r.value[0].data[0],'base64');
}
try{
  await until(async()=>{try{return await rpc.call('getHealth')==='ok';}catch{return false;}},90000,'health');
  const genesisHash=await rpc.call('getGenesisHash'),version=await rpc.call('getVersion');
  const f=fixture({solo:false}),feePayer=Keypair.generate();
  const six=[f.creator,f.founder,f.treasury,...f.recovery];
  assert.equal(new Set(six.map(k=>k.publicKey.toBase58())).size,6);
  assert(!six.some(k=>k.publicKey.equals(feePayer.publicKey)));
  await airdrop(f.creator.publicKey,30000000000);await airdrop(feePayer.publicKey,2000000000);
  const programData=PublicKey.findProgramAddressSync([PROGRAM.toBuffer()],LOADER)[0];
  const seal=new TransactionInstruction({programId:LOADER,data:Buffer.from([4,0,0,0]),keys:[
    {pubkey:programData,isSigner:false,isWritable:true},{pubkey:loaderAuthority.publicKey,isSigner:true,isWritable:false}]});
  await send('seal-loader',[seal],feePayer,[feePayer,loaderAuthority]);
  const mintRent=await rpc.call('getMinimumBalanceForRentExemption',[MINT_SIZE]);
  const tokenRent=await rpc.call('getMinimumBalanceForRentExemption',[ACCOUNT_SIZE]);
  const make=(key,space,lamports)=>SystemProgram.createAccount({fromPubkey:f.creator.publicKey,
    newAccountPubkey:key.publicKey,space,lamports,programId:TOKEN_PROGRAM_ID});
  // Creator pays this transaction; keeping its previously validated packet layout.
  await send('mint-source-supply-and-revoke',[
    make(f.mint,MINT_SIZE,mintRent),createInitializeMint2Instruction(f.mint.publicKey,9,f.creator.publicKey,null),
    make(f.source,ACCOUNT_SIZE,tokenRent),createInitializeAccount3Instruction(f.source.publicKey,f.mint.publicKey,f.creator.publicKey),
    createMintToCheckedInstruction(f.mint.publicKey,f.source.publicKey,f.creator.publicKey,SUPPLY,9),
    createSetAuthorityInstruction(f.mint.publicKey,f.creator.publicKey,AuthorityType.MintTokens,null),
  ],f.creator,[f.creator,f.mint,f.source]);
  f.config.t0=(await readClock(rpc)).now+240n;
  f.identity=boundLaunchIdentity({program:PROGRAM.toBuffer(),creator:f.creator.publicKey.toBuffer(),
    mint:f.mint.publicKey.toBuffer(),founder:f.founder.publicKey.toBuffer(),treasury:f.treasury.publicKey.toBuffer(),
    oracle:f.oracle.publicKey.toBuffer(),specHash:f.specHash,config:f.config});
  f.policy=pda(Buffer.from('launch-v7-policy'),f.identity);
  const binding={genesisHash,policy:f.policy.toBase58(),identityHex:f.identity.toString('hex'),specHashHex:f.specHash.toString('hex'),
    creator:f.creator.publicKey.toBase58(),mint:f.mint.publicKey.toBase58(),founder:f.founder.publicKey.toBase58(),
    treasury:f.treasury.publicKey.toBase58(),oracle:f.oracle.publicKey.toBase58()};
  await send('prepare-immutable-config',[prepareInstruction(f)],feePayer,[feePayer,f.creator]);
  const prepared=await bytesOf(preparationAddress(f));
  assert.equal(prepared.length,757);
  const substitute=openInstruction(f);substitute.keys[2].pubkey=f.oracle.publicKey;
  await reject('substituted-treasury',[substitute],feePayer,[feePayer,f.creator,f.founder,f.oracle,...f.recovery]);
  assert((await bytesOf(preparationAddress(f))).equals(prepared));
  await send('open-six-distinct-roles',[openInstruction(f)],feePayer,[feePayer,...six]);
  await readBoundPolicy(rpc,binding);
  await reject('open-replay',[openInstruction(f)],feePayer,[feePayer,...six]);
  assert((await bytesOf(preparationAddress(f))).equals(prepared));
  const vault=role=>pda(Buffer.from('launch-v7-vault'),f.policy.toBuffer(),Buffer.from([role]));
  const token=role=>pda(Buffer.from('launch-v7-token'),vault(role).toBuffer());
  for(const role of [0,1]){
    const owner=role===0?f.founder:f.treasury;
    await send('deposit-'+role,[instruction('deposit',{creator:f.creator.publicKey,depositor:f.creator.publicKey,
      authority:owner.publicKey,policy:f.policy,mint:f.mint.publicKey,source:f.source.publicKey,
      vault:vault(role),vault_token:token(role),token_program:TOKEN_PROGRAM_ID,system_program:SYSTEM},
    [Buffer.from([role]),integer(role===0?f.config.founder_amount:f.config.treasury_amount)])],feePayer,[feePayer,f.creator,owner]);
  }
  await send('arm',[instruction('arm',{creator:f.creator.publicKey,policy:f.policy})],feePayer,[feePayer,f.creator]);
  await until(async()=>(await readClock(rpc)).now>=f.config.t0,300000,'natural-T0');
  await send('activate',[instruction('activate',{policy:f.policy})],feePayer,[feePayer]);
  const names=['program','program_data','clock','preparation','policy','mint','source','founder_vault','treasury_vault','founder_token','treasury_token'];
  const addresses=[PROGRAM,programData,CLOCK,preparationAddress(f),f.policy,f.mint.publicKey,f.source.publicKey,vault(0),vault(1),token(0),token(1)];
  const {response,clock}=await readClockBoundAccounts(rpc,addresses.map(k=>k.toBase58()));
  assert.equal(await rpc.call('getGenesisHash'),genesisHash);
  const snapshot={program_id:PROGRAM.toBase58(),genesis_hash:genesisHash,slot:response.context.slot,
    now:clock.readBigInt64LE(32).toString(),accounts:Object.fromEntries(names.map((name,i)=>{
      const a=response.value[i];assert(a&&a.data[1]==='base64');
      return [name,{address:addresses[i].toBase58(),owner:a.owner,executable:a.executable,
        lamports:String(a.lamports),data_hex:Buffer.from(a.data[0],'base64').toString('hex')}];
    }))};
  assert.equal(snapshot.accounts.preparation.data_hex,prepared.toString('hex'));
  const expected={program:PROGRAM.toBase58(),genesis_hash:genesisHash,creator:binding.creator,mint:binding.mint,
    founder:binding.founder,treasury:binding.treasury,oracle:binding.oracle,policy:binding.policy,
    preparation:preparationAddress(f).toBase58(),identity:binding.identityHex,spec_hash:binding.specHashHex,
    config_borsh_hex:encodeConfig(f.config).toString('hex'),fee_payer:feePayer.publicKey.toBase58()};
  save('snapshot',snapshot);save('expected',expected);
  const decoded=spawnSync('python3',['src/e11b_bootstrap_verifier.py',join(out,'snapshot.json'),
    join(out,'expected.json'),join(out,'builds.json')],{encoding:'utf8',env:{...process.env,PYTHONPATH:'src'}});
  assert.equal(decoded.status,0,decoded.stdout+decoded.stderr);
  const verified=JSON.parse(decoded.stdout);assert.equal(verified.valid,true);save('verification',verified);
  const receipt={schema:'K4V-E11B-AGAVE-v1',valid:true,scope:'ACTUAL_LOCAL_NODE_NATURAL_CLOCK_BOOTSTRAP',
    program:PROGRAM.toBase58(),program_sha256:CODE_SHA256,genesis_hash:genesisHash,version,
    program_origin:'genesis-preload-then-signed-loader-revocation',six_role_keys:six.map(k=>k.publicKey.toBase58()),
    independent_fee_payer:feePayer.publicKey.toBase58(),human_independent_governance:false,
    finalized_client_transactions:receipts.length,expected_refusals:refusals,raw_snapshot_sha256:hash(readFileSync(join(out,'snapshot.json'))),
    private_keys_serialized:false,account_injection:false,clock_override:false,public_chain_transactions:0,
    long_duration_recovery_execution_verified:false,production_ready:false,human_review:false};
  save('signed-transactions',receipts);save('receipt',receipt);console.log('E11B_AGAVE_RESULT '+JSON.stringify(receipt));
}catch(error){
  save('failure',{message:error.message,stack:error.stack,finalized_transactions:receipts.length});
  save('signed-transactions',receipts);throw error;
}finally{
  try{await stopLocalChild(validator,tracked);}finally{log.end();await finished(log);rmSync(ledger,{recursive:true,force:true});}
}
