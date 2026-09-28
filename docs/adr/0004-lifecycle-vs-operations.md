# ADR-0004: Lifecycle state is separate from active operations

- Status: Accepted
- Date: 2026-09-28

## Context

A single state enum including `BACKING_UP`/`UPDATING` loses the fact that the process is
alive (live backups run while the server is Running) and invites inconsistent states.

## Decision

Each server has a `LifecycleState` (process) and a set of `ActiveOperations` (jobs),
guarded by an `OperationLock` compatibility matrix. The UI shows a combined badge.

## Consequences

Operations are serialized or rejected with a clear reason; background work never
corrupts lifecycle bookkeeping.
