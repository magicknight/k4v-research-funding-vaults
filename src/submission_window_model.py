"""E-09 TEST_ONLY signed-window model; no RPC, SBF repair or real signatures.

Symbolic signer->digest attestations model signatures over one exact intent.
The E-06 authority machine supplies unchanged role/quorum/cancel/expiry rules.
Financial bytes are opaque witnesses, never executed balances or transfers.
"""
from copy import deepcopy
from dataclasses import asdict, dataclass, field
import hashlib
import json

import withdrawal_recovery_model as authority

NOTICE = authority.NOTICE
EXECUTION_WINDOW = authority.EXECUTION_WINDOW
MAX_SUBMISSION_WINDOW = 300  # Frozen TEST_ONLY bound, not a production choice.
DOMAIN = "k4v-e09-submission-window-model-v1"


@dataclass(frozen=True)
class Intent:
    program: str
    policy: str
    identity: str
    role: str
    predecessor: str
    successor: str
    mode: str
    nonce: int
    epoch: int
    valid_from: int
    valid_until: int

    def digest(self):
        payload = {"domain": DOMAIN, "notice": NOTICE, "execution_window": EXECUTION_WINDOW,
                   "max_submission_window": MAX_SUBMISSION_WINDOW, "intent": asdict(self)}
        return hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


@dataclass(frozen=True)
class Admission:
    intent: Intent
    intent_digest: str
    accepted_at: int


@dataclass
class State:
    program: str
    identity: str
    core: authority.State
    admissions: dict = field(default_factory=dict)


def bind(core, program, identity):
    authority.key(program)
    authority.require(type(identity) is str and len(identity) == 64
                      and all(c in "0123456789abcdef" for c in identity), "IDENTITY")
    authority.require(all(r.sequence == 0 and r.pending is None for r in core.roles.values()), "FRESH_MODEL_ONLY")
    return State(program, identity, deepcopy(core))


def validate_intent(intent):
    authority.require(isinstance(intent, Intent), "INTENT_TYPE")
    for k in (intent.program, intent.policy, intent.predecessor, intent.successor):
        authority.key(k)
    authority.require(type(intent.identity) is str and len(intent.identity) == 64
                      and all(c in "0123456789abcdef" for c in intent.identity), "IDENTITY")
    authority.require(intent.role in authority.ROLES and intent.mode in ("normal", "recovery"), "ROLE_OR_MODE")
    authority.number(intent.nonce)
    authority.number(intent.epoch)
    authority.number(intent.valid_from, authority.I64)
    authority.number(intent.valid_until, authority.I64)
    authority.require(0 <= intent.valid_until - intent.valid_from <= MAX_SUBMISSION_WINDOW, "SUBMISSION_WINDOW_WIDTH")
    authority.require(intent.valid_until <= authority.I64 - NOTICE - EXECUTION_WINDOW, "TIME_OVERFLOW")


def prepare(state, role, successor, mode, valid_from, valid_until):
    authority.require(role in authority.ROLES, "ROLE")
    r = state.core.roles[role]
    intent = Intent(state.program, state.core.policy, state.identity, role, r.current,
                    successor, mode, r.sequence + 1, r.epoch, valid_from, valid_until)
    validate_intent(intent)
    return intent


def submit(state, intent, attestations, now):
    validate_intent(intent)
    authority.number(now, authority.I64)
    authority.require((intent.program, intent.policy, intent.identity) ==
                      (state.program, state.core.policy, state.identity), "INTENT_DOMAIN")
    authority.require(intent.valid_from <= now <= intent.valid_until, "SUBMISSION_TIME")
    authority.require(intent.predecessor == state.core.roles[intent.role].current, "PREDECESSOR")
    authority.require(isinstance(attestations, dict), "ATTESTATIONS")
    digest = intent.digest()
    for signer, signed_digest in attestations.items():
        authority.key(signer)
        authority.require(signed_digest == digest, "SIGNED_INTENT_CHANGED")
    core = authority.propose(state.core, intent.role, intent.successor, intent.mode,
                             intent.nonce, intent.epoch, set(attestations), now)
    result = State(state.program, state.identity, core, deepcopy(state.admissions))
    key = (intent.role, intent.nonce)
    authority.require(key not in result.admissions, "ADMISSION_ALREADY_USED")
    result.admissions[key] = Admission(intent, digest, now)
    return result


def pending(state, role, intent_digest):
    authority.require(role in authority.ROLES, "ROLE")
    p = state.core.roles[role].pending
    authority.require(p is not None, "NO_PENDING")
    a = state.admissions.get((role, p.nonce))
    authority.require(a is not None, "ADMISSION_MISSING")
    i = a.intent
    validate_intent(i)
    authority.require(a.intent_digest == i.digest() == intent_digest, "ADMISSION_DIGEST")
    authority.require((i.program, i.policy, i.identity) == (state.program, state.core.policy, state.identity), "INTENT_DOMAIN")
    authority.require((p.policy, p.role, p.nonce, p.epoch, p.predecessor, p.successor, p.mode) ==
                      (i.policy, i.role, i.nonce, i.epoch, i.predecessor, i.successor, i.mode), "ADMISSION_PROPOSAL")
    authority.require(i.valid_from <= a.accepted_at <= i.valid_until
                      and p.created_at == a.accepted_at and p.execute_after == a.accepted_at + NOTICE
                      and p.expires_at == a.accepted_at + NOTICE + EXECUTION_WINDOW, "FULL_NOTICE_FROM_ADMISSION")
    return p


def execute(state, role, digest, now):
    p = pending(state, role, digest)
    return State(state.program, state.identity, authority.execute(state.core, role, p.digest(), now), deepcopy(state.admissions))


def cancel(state, role, digest, signers, now):
    p = pending(state, role, digest)
    return State(state.program, state.identity, authority.cancel(state.core, role, p.digest(), signers, now), deepcopy(state.admissions))


def expire(state, role, digest, now):
    p = pending(state, role, digest)
    return State(state.program, state.identity, authority.expire(state.core, role, p.digest(), now), deepcopy(state.admissions))
