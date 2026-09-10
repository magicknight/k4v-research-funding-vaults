"""Read-only byte verification of v7 bootstrap snapshots; no RPC or IDL required.
The caller supplies the expected binding and separately produced build record.
This verifies bytes, not independent network provenance or human approval.
"""
import argparse
import hashlib
import json
import struct
from pathlib import Path
from beneficiary_vault_verifier import _base58_encode, _pubkey, find_program_address
from launch_v7_verifier import PROGRAM, TOKEN, Reader, account, read_policy, read_vault, read_token, require, validate_config
LOADER='BPFLoaderUpgradeab1e11111111111111111111111'
CLOCK='SysvarC1ock11111111111111111111111111111111'
SYSVAR='Sysvar1111111111111111111111111111111111111'

def verify(snapshot, expected, builds):
    require(snapshot['program_id']==PROGRAM==expected['program'], 'PROGRAM_BINDING')
    require(snapshot['genesis_hash']==expected['genesis_hash'], 'GENESIS_BINDING')
    clock_address, clock_reader=account(snapshot,'clock',SYSVAR)
    require(clock_address==CLOCK and len(clock_reader.data)==40, 'CLOCK_IDENTITY')
    require(struct.unpack_from('<Q',clock_reader.data)[0]==int(snapshot['slot']), 'CLOCK_BANK')
    now=struct.unpack_from('<q',clock_reader.data,32)[0]
    require(now==int(snapshot['now']), 'CLOCK_TIME')
    program=snapshot['accounts']['program']
    require(program['address']==PROGRAM and program['owner']==LOADER and program['executable'] is True,'PROGRAM_ACCOUNT')
    raw=bytes.fromhex(program['data_hex'])
    require(len(raw)==36 and struct.unpack_from('<I',raw)[0]==2,'PROGRAM_LAYOUT')
    address,reader=account(snapshot,'program_data',LOADER)
    code=reader.data
    expected_data,_=find_program_address((_pubkey(PROGRAM),),_pubkey(LOADER))
    require(address==_base58_encode(expected_data)==_base58_encode(raw[4:]),'LOADER_POINTER')
    n=builds['test']['size']
    require(len(code)>=45+n and struct.unpack_from('<I',code)[0]==3 and code[12]==0,'LOADER_AUTHORITY')
    require(struct.unpack_from('<Q',code,4)[0]<=int(snapshot['slot']),'LOADER_SLOT')
    require(hashlib.sha256(code[45:45+n]).hexdigest()==builds['test']['sha256'],'SBF_HASH')
    require(not any(code[45+n:]),'LOADER_PADDING')
    preparation,r=account(snapshot,'preparation',PROGRAM,'LaunchPreparationV7')
    actors={k:r.key() for k in ('creator','mint','founder','treasury','oracle','program')}
    identity,spec_hash=r.take(32),r.take(32)
    config=r.take(492)
    bump=r.number('B');r.finish()
    for key,value in actors.items(): require(value==expected[key],'PREPARATION_ACTOR_'+key)
    require(identity.hex()==expected['identity'] and spec_hash.hex()==expected['spec_hash'],'PREPARATION_BINDING')
    require(config.hex()==expected['config_borsh_hex'],'PREPARATION_CONFIG')
    preimage=b'k4v-launch-policy-v6-test-profile-1'+_pubkey(PROGRAM)
    preimage+=b''.join(_pubkey(actors[k]) for k in ('creator','mint','founder','treasury','oracle'))
    preimage+=spec_hash+struct.pack('<qqQHqBqq',15552000,2592000,12,500,7776000,2,2592000,300)+config
    require(hashlib.sha256(preimage).digest()==identity,'PREPARATION_CONTENT_HASH')
    pk,pb=find_program_address((b'launch-v7-preparation',identity),_pubkey(PROGRAM))
    require(preparation==expected['preparation']==_base58_encode(pk) and bump==pb,'PREPARATION_PDA')
    p=read_policy(snapshot)
    require(p['address']==expected['policy'] and p['identity']==identity and p['spec_hash']==spec_hash,'POLICY_PREPARATION_LINK')
    require(bytes.fromhex(snapshot['accounts']['policy']['data_hex'])[232:724]==config,'POLICY_CONFIG_COPY')
    for key in ('creator','mint','founder','treasury'): require(p[key]==actors[key],'POLICY_ACTOR_'+key)
    require(p['oracle']==p['initial_oracle']==actors['oracle'],'POLICY_ORACLE')
    require(p['state']==2 and p['funded_mask']==3 and now>=p['config']['t0'],'ACTIVE_FUNDED_STATE')
    require(p['founder_released_total']==p['treasury_released_total']==0,'UNEXPECTED_RELEASE')
    mint_address,m=account(snapshot,'mint',TOKEN)
    require(mint_address==actors['mint'] and len(m.data)==82,'MINT_IDENTITY')
    require(struct.unpack_from('<I',m.data,0)[0]==0 and struct.unpack_from('<I',m.data,46)[0]==0,'LIVE_MINT_AUTHORITY')
    supply=struct.unpack_from('<Q',m.data,36)[0]
    require(m.data[44]==9 and m.data[45]==1 and supply==10**18,'SUPPLY_SCALE')
    validate_config(p['config'],supply)
    signers=[actors['creator'],actors['founder'],actors['treasury'],*p['config']['recovery_keys']]
    require(len(set(signers))==6 and expected['fee_payer'] not in signers,'SIX_DISTINCT_TEST_ROLES')
    tokens=[]
    source=read_token(snapshot,'source')
    require(source['mint']==mint_address and source['owner']==actors['creator'],'SOURCE_BINDING')
    tokens.append(source['amount'])
    for role,name,who in ((0,'founder',actors['founder']),(1,'treasury',actors['treasury'])):
        vault=read_vault(snapshot,name+'_vault');token=read_token(snapshot,name+'_token')
        vp,vb=find_program_address((b'launch-v7-vault',_pubkey(p['address']),bytes([role])),_pubkey(PROGRAM))
        tp,_=find_program_address((b'launch-v7-token',vp),_pubkey(PROGRAM))
        require(vault['address']==_base58_encode(vp) and vault['bump']==vb,'VAULT_PDA')
        require(vault['policy']==p['address'] and vault['role']==role and vault['authority']==who and vault['depositor']==actors['creator'],'VAULT_BINDING')
        require(token['address']==_base58_encode(tp) and token['owner']==vault['address'] and token['mint']==mint_address,'CUSTODY_PDA')
        require(token['delegate']==token['close_authority']==0 and token['delegated_amount']==0,'CUSTODY_DELEGATE')
        amount=p['config'][name+'_amount']
        require(vault['principal']==token['amount']==amount and vault['released_total']==0,'FUNDED_PRINCIPAL')
        require(p['withdrawal'][role]['current']==who and p['withdrawal'][role]['epoch']==0,'WITHDRAWAL_AUTHORITY')
        tokens.append(token['amount'])
    require(sum(tokens)==supply,'SUPPLY_CONSERVATION')
    return {'valid':True,'scope':'LOCAL_BOOTSTRAP_BYTES_ONLY','program':PROGRAM,'six_distinct_keys':True,
            'preparation_config_policy_binding':True,'supply_conserved':True,'upgrade_authority':None,
            'clock_overridden':False,'long_duration_execution_verified':False,'human_review':False,'production_ready':False}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('snapshot');parser.add_argument('expected');parser.add_argument('builds')
    a=parser.parse_args()
    try:
        result=verify(*(json.loads(Path(p).read_text()) for p in (a.snapshot,a.expected,a.builds)))
        print(json.dumps(result,sort_keys=True))
    except (KeyError,ValueError,TypeError,struct.error) as error:
        print(json.dumps({'valid':False,'error':str(error)}));raise SystemExit(1)
