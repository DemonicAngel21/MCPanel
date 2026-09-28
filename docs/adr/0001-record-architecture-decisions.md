# ADR-0001: Record architecture decisions

- Status: Accepted
- Date: 2026-09-28

## Context

MCPanel is developed over many phases, partly with AI assistance. Significant decisions
must be discoverable and must not be silently reversed.

## Decision

We record significant architecture decisions as ADRs in `docs/adr/NNNN-title.md`
(Context / Decision / Consequences). ADRs are immutable once accepted; a later ADR may
supersede an earlier one.

## Consequences

Architecture changes require a new ADR and an update to `docs/architecture/README.md`.
