"""E-06 TEST_ONLY authority state machine; no wallet, RPC, token transfer or SBF.

Symbolic signer sets stand in for authenticated transaction signatures. An
authorization result is only one prerequisite for a release: the consumer must
still enforce the unchanged financial kernel. See the normative E-06 spec.
"""
from copy import deepcopy
from dataclasses import asdict, dataclass, field
import hashlib
import json

DAY = 86_400
NOTICE = 90 * DAY
EXECUTION_WINDOW = 30 * DAY
PERIOD = 30 * DAY
U64 = 2**64 - 1
I64 = 2**63 - 1
ROLES = ("founder", "treasury")


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def number(value, upper=U64):
    require(type(value) is int and 0 <= value <= upper, "INTEGER_RANGE")


def key(value):
    require(type(value) is str and bool(value.strip()), "KEY")


@dataclass(frozen=True)
class Approval:
    period: int
    recipient: str
    owner: str
    need: int
    consumed: int
    created_at: int
    author: str
    author_epoch: int


@dataclass(frozen=True)
class Proposal:
    policy: str
    role: str
    nonce: int
    epoch: int
    predecessor: str
    successor: str
    mode: str
    created_at: int
    execute_after: int
    expires_at: int

    def digest(self):
        encoded = json.dumps(asdict(self), sort_keys=True, separators=(",", ":"))
        return hashlib.sha256(b"k4v-e06-withdrawal-model-v1\0" + encoded.encode()).hexdigest()


@dataclass
class RoleState:
    initial: str
    current: str
    committee: tuple
    history: list
    epoch: int = 0
    sequence: int = 0
    pending: Proposal | None = None
    tombstones: dict = field(default_factory=dict)


@dataclass
class State:
    policy: str
    controller: str
    t0: int
    # Opaque, frozen financial/account identity witness, never a transfer engine.
    economics: bytes
    roles: dict
    approvals: dict = field(default_factory=dict)
    last_at: int = 0


def register(policy, controller, t0, economics, actors, committees, signers):
    """Model registration consent, as required at new-policy creation in E-07."""
    key(policy)
    key(controller)
    number(t0, I64)
    require(type(economics) is bytes and bool(economics), "ECONOMIC_WITNESS")
    require(set(actors) == set(committees) == set(ROLES), "ROLE_SET")
    roles = {}
    for role in ROLES:
        actor, members = actors[role], tuple(committees[role])
        key(actor)
        for member in members:
            key(member)
        require(len(members) == len(set(members)) == 3, "COMMITTEE")
        require(actor not in members, "ACTOR_IS_GUARDIAN")
        require(actor in signers, "REGISTRATION_CONSENT")
        roles[role] = RoleState(actor, actor, members, [actor])
    return State(policy, controller, t0, economics, roles)


def context(state, role, now):
    require(role in ROLES, "ROLE")
    number(now, I64)
    require(now >= state.last_at, "CLOCK_BACKWARDS")
    return state.roles[role]


def quorum(role, signers):
    return len(set(signers).intersection(role.committee)) >= 2


def forbidden_recipients(state):
    blocked = set()
    for role in state.roles.values():
        blocked.update(role.history)
        blocked.update(role.committee)
        if role.pending:
            blocked.add(role.pending.successor)
    return blocked


def successor_allowed(state, role, successor):
    key(successor)
    require(successor not in role.history, "REUSED_WITHDRAWAL_KEY")
    require(successor not in role.committee, "ACTOR_IS_GUARDIAN")
    require(all(a.owner != successor for a in state.approvals.values()),
            "SUCCESSOR_IS_APPROVED_RECIPIENT")


def propose(state, role, successor, mode, nonce, epoch, signers, now):
    r = context(state, role, now)
    number(nonce)
    number(epoch)
    require(r.pending is None, "PENDING")
    require(r.sequence < U64 and nonce == r.sequence + 1, "NONCE")
    require(epoch == r.epoch and epoch < U64, "EPOCH")
    require(mode in ("normal", "recovery"), "MODE")
    successor_allowed(state, r, successor)
    require(successor in signers, "SUCCESSOR_ACCEPTANCE")
    require(r.current in signers if mode == "normal" else quorum(r, signers), "PROPOSAL_AUTHORITY")
    require(now <= I64 - NOTICE - EXECUTION_WINDOW, "TIME_OVERFLOW")
    result = deepcopy(state)
    target = result.roles[role]
    target.pending = Proposal(state.policy, role, nonce, epoch, r.current,
                              successor, mode, now, now + NOTICE,
                              now + NOTICE + EXECUTION_WINDOW)
    target.sequence = nonce
    result.last_at = now
    return result


def pending(state, role, digest, now):
    r = context(state, role, now)
    p = r.pending
    require(p is not None, "NO_PENDING")
    require(p.digest() == digest, "PROPOSAL_BINDING")
    require(p.policy == state.policy and p.role == role and p.epoch == r.epoch
            and p.predecessor == r.current and p.nonce == r.sequence, "STALE_PROPOSAL")
    return r, p


def finish(state, role, p, status, now):
    result = deepcopy(state)
    r = result.roles[role]
    r.tombstones[p.nonce] = {"proposal": asdict(p), "digest": p.digest(),
                             "status": status, "finished_at": now}
    r.pending = None
    result.last_at = now
    return result


def cancel(state, role, digest, signers, now):
    r, p = pending(state, role, digest, now)
    require(p.successor in signers or quorum(r, signers)
            or (p.mode == "normal" and r.current in signers), "CANCEL_AUTHORITY")
    return finish(state, role, p, "cancelled", now)


def expire(state, role, digest, now):
    _, p = pending(state, role, digest, now)
    require(now >= p.expires_at, "NOT_EXPIRED")
    return finish(state, role, p, "expired", now)


def execute(state, role, digest, now):
    r, p = pending(state, role, digest, now)
    require(p.execute_after <= now < p.expires_at, "EXECUTION_WINDOW")
    successor_allowed(state, r, p.successor)
    result = finish(state, role, p, "executed", now)
    target = result.roles[role]
    target.current = p.successor
    target.history.append(p.successor)
    target.epoch += 1
    return result


def authenticate(state, role, signer, epoch, now):
    r = context(state, role, now)
    number(epoch)
    require(r.pending is None, "ROLE_PAUSED")
    require(signer == r.current and epoch == r.epoch, "WITHDRAWAL_AUTHORITY")


def approve(state, signer, epoch, period, recipient, owner, need, now):
    authenticate(state, "treasury", signer, epoch, now)
    number(period)
    number(need)
    key(recipient)
    key(owner)
    require(need > 0, "NEED")
    require(owner not in forbidden_recipients(state), "KNOWN_SELF_PAYMENT")
    require(period not in state.approvals, "APPROVAL_IMMUTABLE")
    end = state.t0 + (period + 1) * PERIOD
    require(end <= I64 and now + PERIOD < end, "APPROVAL_NOTICE")
    require(now < state.t0 or period > (now - state.t0) // PERIOD, "FUTURE_PERIOD")
    result = deepcopy(state)
    result.approvals[period] = Approval(period, recipient, owner, need, 0, now, signer, epoch)
    result.last_at = now
    return result


def authorize_release(state, role, signer, epoch, recipient, owner, now, period=None):
    """Check authority/recipient/approval only. Never sufficient to move funds.

    Caller must also validate real signatures and SPL accounts, lifecycle,
    T0/cliff, reports, principal and period/annual/reserved caps atomically.
    """
    authenticate(state, role, signer, epoch, now)
    key(recipient)
    key(owner)
    if role == "founder":
        require(owner == signer and period is None, "FOUNDER_DESTINATION")
    else:
        number(period)
        require(now >= state.t0 and period == (now - state.t0) // PERIOD, "APPROVAL_PERIOD")
        require(period in state.approvals, "APPROVAL_REQUIRED")
        a = state.approvals[period]
        require((recipient, owner) == (a.recipient, a.owner), "APPROVAL_DESTINATION")
        require(owner not in forbidden_recipients(state), "KNOWN_SELF_PAYMENT")
        require(now >= a.created_at + PERIOD, "APPROVAL_NOTICE")
    return {"policy": state.policy, "role": role, "authority_epoch": epoch,
            "recipient": recipient, "financial_kernel_required": True,
            "transfer_executed": False}
