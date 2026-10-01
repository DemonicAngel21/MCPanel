# Repository Guidelines

## Project Structure & Module Organization

MCPanel is a Rust workspace with a Tauri desktop app. `apps/desktop/src` contains the React/TypeScript UI; `apps/desktop/src-tauri` hosts the Tauri integration. Rust crates under `crates/` separate core logic, application API, database, providers, and platform adapters. `tools/fake-mc` provides a fake server and integration-test harness. Shared assets and data live in `assets/` and `data/`; user documentation, architecture notes, and decisions live in `docs/user/`, `docs/architecture/`, and `docs/adr/`.

## Build, Test, and Development Commands

- `pnpm install` installs JavaScript workspace dependencies.
- `pnpm dev` starts the desktop app with Vite and the Rust host.
- `pnpm build` creates the production app and Windows installer.
- `pnpm check` runs frontend linting, type checking, and Vitest tests.
- `cargo fmt --all` formats Rust; `cargo clippy --workspace --all-targets -- -D warnings` checks it.
- `cargo test --workspace` runs Rust tests; `pnpm gen:bindings` regenerates TypeScript API bindings after DTO changes.
- `cargo deny check` checks dependency advisories, licenses, and sources.

See `docs/development.md` for Windows prerequisites, isolated data setup, and opt-in tests that download or run real Minecraft servers.

## Coding Style & Naming Conventions

Use Rust 2024 and `rustfmt.toml` (100-column width); run `cargo fmt --all`. The frontend uses TypeScript, ESLint, and Prettier (`pnpm --filter @mcpanel/desktop format`). Follow nearby naming patterns: Rust modules and files in `snake_case`, React components in `PascalCase`, and variables/functions in `camelCase`. Keep server-management logic in the Rust layers; the UI calls the typed client in `apps/desktop/src/lib/api`.

## Testing Guidelines

Add Rust tests alongside crate code or under `tools/fake-mc/tests/`; frontend tests use Vitest. Name integration tests by the behavior they cover, such as `lifecycle.rs`. Run focused tests while iterating, then `cargo test --workspace` and `pnpm check` for relevant changes. Real-server and live-provider tests are opt-in; consult `docs/development.md` before running them.

## Commit & Pull Request Guidelines

Use Conventional Commits, matching history (for example, `feat(ui): ...`, `fix: ...`, `docs: ...`). Branches generally use `feature/*`, `fix/*`, `refactor/*`, or `docs/*`. Update `CHANGELOG.md` under `[Unreleased]` for user-visible changes. PRs should explain the change and verification, link related issues, include screenshots for UI changes, and call out migrations or security implications. Never edit released migrations; add a new migration instead.

## Architecture & Security

Read `docs/architecture/README.md` before architecture changes and the relevant ADRs. Preserve the UI → API → core → adapter layering. Route server file access through `SafePath`; launch processes only from core-built `LaunchSpec` arguments, never a shell. Do not log, serialize, emit, or audit secrets. Keep credentials out of Git and do not add telemetry.
