"""Independent fixed-width preparation binding; supplied bytes are not a chain proof."""
import hashlib
import json
from pathlib import Path
import struct
import sys
from beneficiary_vault_verifier import _base58_encode, _pubkey, find_program_address
from launch_v7_verifier import PROGRAM, TOKEN, account, require


def verify_preparation(snapshot, policy=None, expected=None):
    address, r = account(snapshot, 'preparation', PROGRAM, 'LaunchPreparationV7')
    require(len(r.data) == 725, 'PREPARATION_SIZE')
    result = {name: r.key() for name in ('creator', 'mint', 'founder', 'treasury', 'oracle')}
    identity, spec_hash, config, bump = r.take(32), r.take(32), r.take(492), r.number('B')
    r.finish()
    constants = struct.pack('<qqQHqBqq', 15552000, 2592000, 12, 500, 7776000, 2, 2592000, 300)
    preimage = b'k4v-launch-policy-v7-test-profile-1' + _pubkey(PROGRAM)
    preimage += b''.join(_pubkey(result[k]) for k in ('creator', 'mint', 'founder', 'treasury', 'oracle'))
    preimage += spec_hash + constants + config
    require(hashlib.sha256(preimage).digest() == identity, 'PREPARATION_IDENTITY')
    pda, expected_bump = find_program_address((b'launch-v7-preparation', identity), _pubkey(PROGRAM))
    require(address == _base58_encode(pda) and bump == expected_bump, 'PREPARATION_PDA')
    policy_address = _base58_encode(find_program_address((b'launch-v7-policy', identity), _pubkey(PROGRAM))[0])
    mint_address, m = account(snapshot, 'mint', TOKEN)
    require(mint_address == result['mint'] and len(m.data) == 82 and m.data[45] == 1, 'PREPARATION_MINT')
    require(struct.unpack_from('<I', m.data)[0] == 0 and struct.unpack_from('<I', m.data, 46)[0] == 0,
            'PREPARATION_MINT_AUTHORITIES')
    founder, treasury = struct.unpack_from('<QQ', config, 8)
    require(founder + treasury <= struct.unpack_from('<Q', m.data, 36)[0], 'PREPARATION_SUPPLY')
    if policy is not None:
        require(policy['address'] == policy_address and policy['identity'] == identity
                and policy['spec_hash'] == spec_hash, 'PREPARATION_POLICY_BINDING')
        require(all(policy[k] == result[k] for k in ('creator', 'mint', 'founder', 'treasury'))
                and policy['initial_oracle'] == result['oracle'], 'PREPARATION_ACTORS')
        require(bytes.fromhex(snapshot['accounts']['policy']['data_hex'])[232:724] == config,
                'PREPARATION_CONFIG_CHANGED')
    if expected is not None:
        require(all(expected[k] == result[k] for k in ('creator', 'mint', 'founder', 'treasury', 'oracle'))
                and expected['identityHex'] == identity.hex() and expected['specHashHex'] == spec_hash.hex()
                and expected['policy'] == policy_address, 'PREPARATION_EXPECTED_BINDING')
    return {'valid': True, 'preparation': address, 'policy': policy_address, 'identity': identity.hex(),
            't0': str(struct.unpack_from('<q', config)[0]), 'immutable_config_bytes': 492,
            'public_chain_authenticity_verified': False, 'independent_human_audit': False}


if __name__ == '__main__':
    try:
        source = json.loads(Path(sys.argv[1]).read_text())
        result = verify_preparation(source['snapshot'], expected=source['expected'])
    except (ValueError, KeyError, TypeError, struct.error) as e:
        print(json.dumps({'valid': False, 'error': str(e)}))
        raise SystemExit(1) from e
    print(json.dumps(result))
