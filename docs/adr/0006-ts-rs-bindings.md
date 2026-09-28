# ADR-0006: TypeScript bindings generated with ts-rs

- Status: Accepted
- Date: 2026-09-28

## Context

The specification proposed `specta`/`tauri-specta` (marked VERIFY). At implementation
time `tauri-specta` is still a release candidate (2.0.0-rc.x), while `ts-rs` is stable
(12.x).

## Decision

API DTOs in `mcpanel-api` derive `ts_rs::TS`; bindings are exported to
`apps/desktop/src/bindings/` by `cargo test -p mcpanel-api export_bindings` (wrapped by
`pnpm gen:bindings`). The typed IPC client in the UI is a small hand-written layer
over `invoke` whose argument and return types come from the generated bindings. CI
fails if generated bindings are out of date.

## Consequences

No dependency on a pre-release crate. Command names are kept in sync by a single
`commands.ts` table plus a Rust test listing registered command names.
