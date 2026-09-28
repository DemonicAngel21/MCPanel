# MCPanel — guidance for AI-assisted development

Read `docs/architecture/README.md` (the approved specification) before changing
architecture. Significant decisions live in `docs/adr/`. External API facts live in
`docs/architecture/verification-log.md` — verify before relying on an external API.

## Architecture rules (non-negotiable)

- Layering: UI → Application API (`crates/mcpanel-api`) → Core (`crates/mcpanel-core`)
  → adapters (`mcpanel-db`, `mcpanel-providers`, `mcpanel-platform`).
- `mcpanel-core` must not depend on Tauri, SQLx, reqwest or the `windows` crate.
- The UI (`apps/desktop/src`) contains no server-management logic; it calls the typed
  client in `src/lib/api`.
- All server file access goes through `SafePath` (`mcpanel-core::files`).
- Processes are spawned only from a core-built `LaunchSpec`, argv only, never via a shell.
- Secrets use secret types; never log, serialise, emit or audit them.
- Minecraft config files are the source of truth for Minecraft config.
- Schema changes = new migration file; never edit released migrations.
- Never fake functionality. Unimplemented = absent or visibly marked.
- No telemetry.

## Commands

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm gen:bindings        # regenerate TS bindings from Rust DTOs
pnpm lint; pnpm typecheck; pnpm test
pnpm dev                 # run the app
pnpm build               # production build + NSIS installer
```

## Workflow

Conventional Commits; `feature/*`, `fix/*`, `refactor/*`, `docs/*` branches; update
`CHANGELOG.md` under `[Unreleased]`. Do not push, create remote repositories, or publish
releases without explicit instruction. Report changes: files created/modified/deleted,
tests added/run, migrations, security considerations, commit message.
