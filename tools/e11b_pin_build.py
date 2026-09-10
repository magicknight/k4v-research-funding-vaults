#!/usr/bin/env python3
"""Record an initial TEST_ONLY build, then require byte-exact replay thereafter."""
from pathlib import Path
import hashlib
import json
R = Path(__file__).resolve().parents[1]
record = {'schema': 'K4V-LAUNCH-V7-BUILD-v1', 'program': 'CYFsfATtQB3Excjsm4Cuh8ZWnPE5j6XAU3GS3RKXmUcK', 'solana_cli': '3.1.10', 'platform_tools': 'v1.52', 'production_ready': False, 'profiles': {}}
for profile in ('disabled', 'test'):
    data = (R / f'target/v7-{profile}/launch_vault_v7.so').read_bytes()
    assert data.startswith(b'\x7fELF') and len(data) > 100000, 'NOT_SBF_ELF'
    record['profiles'][profile] = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
p = R / 'spec/LAUNCH_V7_BUILD_IDENTITY_v1.json'
if p.exists():
    assert json.loads(p.read_text()) == record, 'FROZEN_V7_BUILD_DRIFT'
else:
    p.write_text(json.dumps(record, indent=2) + '\n')
print(json.dumps(record))
