# Contributing to MCPanel

Thanks for your interest in MCPanel! This document describes how the project is
developed. Please read [`docs/architecture`](docs/architecture/README.md) before making
non-trivial changes.

## Ground rules

1. **Respect the architecture.** UI → Application API → Core → Providers/Adapters.
   - The UI contains no server-management logic.
   - `mcpanel-core` must not depend on Tauri, SQLx, reqwest or Win32 in its public API.
   - External services live behind provider traits.
   - Minecraft configuration files are the source of truth for Minecraft config; the
     database stores MCPanel's own data.
2. **Security first.** All file access goes through `SafePath`. No shell execution.
   Secrets use secret types and never reach logs, events, audit records or the UI.
3. **No fake functionality.** Do not add buttons that pretend to work. If something is
   not implemented, it is visibly marked as such (or absent).
4. **No telemetry.** Any future crash reporting must be explicit opt-in.

## Branching

| Branch | Purpose |
|---|---|
| `main` | Stable, always releasable. Protected: PR + green CI required. |
| `feature/<name>` | New features (`feature/backup-system`) |
| `fix/<name>` | Bug fixes (`fix/console-freeze`) |
| `refactor/<name>` | Refactoring without behaviour change |
| `docs/<name>` | Documentation |
| `chore/<name>`, `security/<name>` | Maintenance, security hardening |

PRs are squash-merged; the PR title becomes the commit message.

## Commit messages — Conventional Commits

```
<type>(<optional scope>): <description>
```

Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `security`, `chore`, `ci`,
`build`. Examples:

```
feat(server): add Paper server provider
fix(console): prevent UI freeze on output floods
security(files): reject NTFS alternate data streams in paths
```

Meaningless messages ("stuff", "update", "fixed things") are not accepted.

## Changelog

Every user-visible change adds an entry under `## [Unreleased]` in
[CHANGELOG.md](CHANGELOG.md) (Added / Changed / Fixed / Security / Removed).

## Local checks

Run before opening a PR:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
pnpm lint
pnpm typecheck
pnpm test
```

## Database changes

Schema changes **must** be new migration files in `crates/mcpanel-db/migrations/`.
Never edit a migration that has been released. Add a migration test.

## AI-assisted development workflow

Changes made with AI assistance follow the same rules. Before changing architecture:

1. Inspect the existing architecture and identify affected modules.
2. Explain the proposed change and check compatibility.
3. Implement, then run the appropriate tests/build checks.
4. Update documentation.
5. Report: what changed, files created/modified/deleted, tests added/run, migrations,
   security considerations, and the recommended commit message.

Do not rewrite unrelated parts of the project or introduce a different architecture for
a single feature.

## Secrets

Never commit secrets, credentials, API tokens, OAuth data, encryption keys, Playit
credentials, Floodgate keys or `.env` files. If a secret is committed accidentally:

1. Stop. 2. Inform the maintainers. 3. **Revoke/rotate the secret first.**
4. Remove it from the working tree. 5. Remove it from history (`git filter-repo`) when
necessary.

## Licensing of contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in MCPanel by you, as defined in the Apache-2.0 license, shall be dual licensed
under MIT OR Apache-2.0, without any additional terms or conditions.
