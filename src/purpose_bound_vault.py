# SPDX-License-Identifier: MIT OR Apache-2.0
"""Executable covenant for purpose-bound research-funding vaults.

This module is chain-independent. It is a deterministic reference model and
audit-receipt generator, not a production smart contract or security audit.
All token amounts use integer base units and all rates use basis points.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
from enum import Enum
import hashlib
import json
import re
from typing import Iterable


BPS_DENOMINATOR = 10_000
MONTHS_PER_YEAR = 12
RECEIPT_SCHEMA = "purpose-bound-vault-receipt/v0.1"


class EventKind(str, Enum):
    SALE = "sale"
    OTC = "otc"
    GRANT = "grant"
    FREE_WALLET_TRANSFER = "free_wallet_transfer"
    COLLATERAL = "collateral"
    ECONOMIC_RIGHT_TRANSFER = "economic_right_transfer"
    EQUAL_OR_STRICTER_LOCK_MIGRATION = "equal_or_stricter_lock_migration"


@dataclass(frozen=True)
class ReleaseEvent:
    kind: EventKind
    amount: int

    def __post_init__(self) -> None:
        if self.amount < 0:
            raise ValueError("event amount must be non-negative")


@dataclass(frozen=True)
class ReleaseRequest:
    period_id: str
    beneficiary_locked_balance_at_year_start: int
    purpose_locked_balance_at_year_start: int
    annual_release_bps: int
    eligible_trailing_30d_spot_volume: int
    market_capacity_bps: int
    beneficiary_months_since_genesis: int
    beneficiary_cliff_months: int = 24
    beneficiary_events: tuple[ReleaseEvent, ...] = ()
    purpose_events: tuple[ReleaseEvent, ...] = ()
    purpose_approved_need: int = 0

    def __post_init__(self) -> None:
        if re.fullmatch(r"[0-9]{4}-(0[1-9]|1[0-2])", self.period_id) is None:
            raise ValueError("period_id must use YYYY-MM")
        integer_fields = (
            self.beneficiary_locked_balance_at_year_start,
            self.purpose_locked_balance_at_year_start,
            self.eligible_trailing_30d_spot_volume,
            self.beneficiary_months_since_genesis,
            self.beneficiary_cliff_months,
            self.purpose_approved_need,
        )
        if any(value < 0 for value in integer_fields):
            raise ValueError("balances, volume, time, cliff, and need must be non-negative")
        if not 0 <= self.annual_release_bps <= 500:
            raise ValueError("annual release rate must be between 0 and 500 bps")
        if not 0 <= self.market_capacity_bps <= 500:
            raise ValueError("market capacity rate must be between 0 and 500 bps")


@dataclass(frozen=True)
class ReleaseDecision:
    allowed: bool
    reasons: tuple[str, ...]
    beneficiary_net_release: int
    purpose_net_release: int
    beneficiary_monthly_rate_cap: int
    purpose_monthly_rate_cap: int
    aggregate_market_capacity: int


def net_release(events: Iterable[ReleaseEvent]) -> int:
    """Count circulation-equivalent events; exempt only stricter lock migration."""

    return sum(
        event.amount
        for event in events
        if event.kind is not EventKind.EQUAL_OR_STRICTER_LOCK_MIGRATION
    )


def _monthly_rate_cap(balance: int, annual_release_bps: int) -> int:
    return (balance * annual_release_bps // BPS_DENOMINATOR) // MONTHS_PER_YEAR


def evaluate_release(request: ReleaseRequest) -> ReleaseDecision:
    """Evaluate one month; unused capacity is intentionally not an input."""

    beneficiary_release = net_release(request.beneficiary_events)
    purpose_release = net_release(request.purpose_events)
    beneficiary_cap = _monthly_rate_cap(
        request.beneficiary_locked_balance_at_year_start,
        request.annual_release_bps,
    )
    purpose_cap = _monthly_rate_cap(
        request.purpose_locked_balance_at_year_start,
        request.annual_release_bps,
    )
    market_cap = (
        request.eligible_trailing_30d_spot_volume
        * request.market_capacity_bps
        // BPS_DENOMINATOR
    )

    reasons: list[str] = []
    if (
        request.beneficiary_months_since_genesis
        < request.beneficiary_cliff_months
        and beneficiary_release > 0
    ):
        reasons.append("BENEFICIARY_CLIFF")
    if beneficiary_release > beneficiary_cap:
        reasons.append("BENEFICIARY_MONTHLY_RATE_CAP")
    if purpose_release > purpose_cap:
        reasons.append("PURPOSE_MONTHLY_RATE_CAP")
    if purpose_release > request.purpose_approved_need:
        reasons.append("PURPOSE_APPROVED_NEED")
    if beneficiary_release + purpose_release > market_cap:
        reasons.append("AGGREGATE_MARKET_CAPACITY")

    return ReleaseDecision(
        allowed=not reasons,
        reasons=tuple(reasons),
        beneficiary_net_release=beneficiary_release,
        purpose_net_release=purpose_release,
        beneficiary_monthly_rate_cap=beneficiary_cap,
        purpose_monthly_rate_cap=purpose_cap,
        aggregate_market_capacity=market_cap,
    )


def request_from_dict(data: dict) -> ReleaseRequest:
    """Parse a JSON-compatible request while rejecting unknown event kinds."""

    values = dict(data)
    for field in ("beneficiary_events", "purpose_events"):
        values[field] = tuple(
            ReleaseEvent(EventKind(item["kind"]), int(item["amount"]))
            for item in values.get(field, [])
        )
    return ReleaseRequest(**values)


def _jsonable(value):
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, tuple):
        return [_jsonable(item) for item in value]
    if isinstance(value, list):
        return [_jsonable(item) for item in value]
    if isinstance(value, dict):
        return {key: _jsonable(item) for key, item in value.items()}
    return value


def decision_receipt(request: ReleaseRequest) -> dict:
    """Return a canonical decision plus a hash over schema, input, and output."""

    decision = evaluate_release(request)
    body = {
        "schema": RECEIPT_SCHEMA,
        "request": _jsonable(asdict(request)),
        "decision": _jsonable(asdict(decision)),
    }
    canonical = json.dumps(body, sort_keys=True, separators=(",", ":"))
    return {
        **body,
        "sha256": hashlib.sha256(canonical.encode("utf-8")).hexdigest(),
    }
