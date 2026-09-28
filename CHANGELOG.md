# Changelog

All notable changes to MCPanel are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Repository foundation: documentation, architecture specification, ADRs, licensing
  (MIT OR Apache-2.0), contribution and security policies.
- Headless Rust core (`mcpanel-core`): server lifecycle state machine separate from
  active operations, process supervision with readiness detection, crash/exit
  classification with diagnoses (port in use, EULA, Java too old, out of memory…),
  graceful stop with timed forced termination, restart, and detection of servers left
  running by a previous MCPanel session ("detached").
- Console pipeline: per-server ring buffer, capture files, batched gap-aware streaming,
  log-level parsing, player join/leave tracking from the log.
- Safe file management: `SafePath` (traversal, drive/UNC, NTFS streams, device names,
  8.3 aliases, junction/symlink escapes), safe ZIP extraction (zip-slip, bombs,
  case collisions, staging), trash, atomic writes, encoding detection/preservation,
  conflict detection, protection of sensitive key files (Floodgate keys, Geyser tokens).
- Lossless, version-aware `server.properties` editing (schema for the supported
  Minecraft versions, unknown keys preserved, sensitive values write-only).
- Java runtime discovery (JAVA_HOME, PATH, registry, vendor folders, Minecraft Launcher
  runtimes), validation by executing the runtime, compatibility checks.
- Server software providers: Vanilla (Mojang manifest), Paper (Fill v3), Purpur — with
  HTTPS-only, host-allow-listed, hash-verified downloads.
- SQLite persistence with guarded forward-only migrations and pre-migration backups.
- Windows platform adapter: Job Object process trees, minimal child environment,
  port-owner lookup, disk space, OneDrive/system-folder location checks, metrics.
- Application API with typed DTOs (TypeScript bindings via ts-rs), stable error codes,
  per-call authorization, and single-use path grants for native dialogs.
- Tauri desktop app: tray (close-to-tray, per-server start/stop, quit prompt with
  graceful stop), crash notifications, drag-and-drop uploads.
- React UI: dashboard, servers, create-server wizard with explicit EULA consent,
  import, server overview, virtualized console, file explorer, Monaco editor,
  properties editor, launch settings, activity log, Java runtimes, settings, Ctrl+K
  command palette, dark and light themes.
- Audit log of user and system actions (no secrets).
- CI (format, lint, types, tests, bindings freshness, unsigned installer) and security
  workflows (cargo-deny, pnpm audit, gitleaks, CodeQL), Dependabot, pluggable signing.
- Test tooling: `fake-mc` test double, end-to-end lifecycle tests, `mcpanel-seed` for
  isolated UI runs, opt-in real Minecraft server test (requires explicit EULA opt-in).

- Local backups (v0.2): plain ZIP archives with a manifest of SHA-256 hashes, live
  backups of running servers (`save-off` / `save-all flush` / `save-on`), verification,
  restore with preview, a protected pre-restore backup and rollback, per-server
  schedules that can skip idle servers, and GFS retention. Backups that contain the
  Floodgate key are flagged as sensitive. New Backups page and server Backups tab;
  backups go to `%USERPROFILE%\MCPanel\Backups` by default (configurable).
- Player management (v0.2): Players tab with online players, known players (play time,
  last seen), whitelist, operators, player and IP bans, kick and a whitelist switch.
  While a server runs, changes are console commands and the server's reply is shown;
  while it is stopped, MCPanel edits `ops.json` / `whitelist.json` / `banned-*.json`
  directly (names resolved via the user cache, the Mojang profile service in online
  mode, or the offline UUID). Play sessions are recorded; IP addresses are not.
- Job progress events carry the job kind, so every job shows a meaningful toast.

### Fixed

- Minecraft 26.x player joins and leaves (logged as `System chat: …`) are detected.

- The database is closed on quit (checkpointing the WAL) and when an open is refused
  (for example a schema from a newer MCPanel), so the file is released.
- Integration tests release their databases and verify their temporary directories
  are deleted, instead of silently leaving them in `%TEMP%` on Windows.

### Security

- No network listener, no telemetry, strict CSP, argv-only process spawning, redacted
  logs, sensitive-file guard.
