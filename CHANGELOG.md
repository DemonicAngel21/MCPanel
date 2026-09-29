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
- Plugin manager (v0.2): Plugins tab to browse Modrinth and Hangar (filtered to the
  server's software and Minecraft version), install with required dependencies after a
  plan preview, update, disable/enable and remove. Downloads come only from the
  providers' CDNs, are checked against the published SHA-512/SHA-256 and must contain a
  `plugin.yml`/`paper-plugin.yml`; replaced files go to the server's trash and a failed
  install is rolled back. Changes to a running server are queued and applied when it
  stops or right before it starts ("Apply & restart"). Plugins added by hand are
  identified on Modrinth by their hash.
- Crash auto-restart (v0.2): crashes are recorded with the exit code, a classification
  (out of memory, watchdog, crash report, …), the crash report path and the console tail
  (Recent crashes on the Overview), and restarted per server policy: by default up to
  3 times within 10 minutes, 10 s delay doubled per attempt, reset after 5 minutes of
  uptime, optional backup first. Failures a restart cannot fix (wrong Java, EULA, port
  in use) are never restarted.
- Server templates (v0.2): five built-in templates (Survival, Friends only, Creative
  build world, Hardcore, Large world tuned) on a new Templates page. The create wizard is
  filled in from a template (software, memory, version-appropriate properties validated
  against the property schema), shows what else it sets, and after creation applies its
  backup schedule, restart policy and chosen suggested plugins.
- Servers that fail because Java rejects a JVM argument ("Unrecognized VM option") get
  a clear diagnosis and are not restarted in a loop.
- Fabric support (v0.2): create Fabric servers (Fabric Loader from the Fabric meta API).
  MCPanel installs Fabric itself so every file is verified — the Mojang server jar by
  SHA-1 and each Fabric library by SHA-512 (Fabric's own installer does not verify
  libraries) — and launches Fabric Loader with the libraries on the class path. Mods
  from Modrinth on a Mods tab: client-only mods are hidden and refused.
- Forge and NeoForge support (v0.3): the official installer (SHA-512 verified from the
  Forge/NeoForge Maven) runs with the selected Java in the server folder and is removed
  afterwards; 1.17+ servers start from the installer's argument file, older Forge from
  its server jar. Forge picks the recommended build by default. Mods for both come from
  Modrinth.
- Quilt support (v0.3): installed natively like Fabric (Mojang server jar + SHA-512-
  verified libraries from Quilt's and Fabric's Maven); runs Quilt and Fabric mods from
  Modrinth.
- Bedrock crossplay (v0.3): a Bedrock tab sets up Geyser (+ Floodgate, + ViaVersion when
  the server is older than Geyser's Minecraft version) on Paper/Purpur from GeyserMC's
  download API (SHA-256) and on Fabric/NeoForge from Modrinth; seeds and patches the
  Geyser config (port, sign-in method) without touching other options; refuses to start
  when another program holds the Bedrock UDP port; tests the listener with a RakNet
  ping. The Floodgate key is only reported as present, never read.
- Backup encryption (v0.4): age (X25519) with a Backup Master Key kept in the Windows
  Credential Manager (this user, this computer). Setting it up requires saving a
  Recovery Kit — the key protected by a passphrase (age scrypt) — which restores access
  on another computer. New backups are then written as `.zip.age`; verify and restore
  decrypt to a temporary file next to the backup that is always removed. Development
  instances with their own data folder use a separate Credential Manager namespace.
- SpigotMC plugins through Spiget (v0.4): search SpigotMC resources and install the
  latest version of free resources from Spiget's CDN. Spiget publishes no hashes, so
  these downloads are marked unverified; premium and external resources link to their
  page instead. Download redirects are resolved first and only Spiget's CDN is accepted.
- TPS and MSPT (v0.4): running servers are asked every 15 s with `tick query`
  (Minecraft 1.20.3+, including Fabric/Quilt/NeoForge/Forge) or Paper/Purpur's `tps` and
  `mspt`; the overview shows the values with 30-minute sparklines. Vanilla TPS is
  labelled as calculated from MSPT. The replies are kept out of the console view; a
  server that does not answer is not asked again until it restarts. Can be turned off
  in Settings.
- Notifications (v0.3): an inbox (bell in the rail, unread count, mark read, clear) and
  Windows desktop notifications for crashes (with what the restart policy did), failed
  backups, other failed tasks and — opt-in — player joins. Settings has a rule per
  category for inbox and desktop. Replaces the fixed crash-only desktop notification.
- Internet access card (v0.3): detects an installed playit.gg agent and shows its
  version and state through the official `playit` CLI, with links to the playit.gg
  download page and dashboard. Tunnels are created in the dashboard (no supported API
  exists; ADR-0007).
- Server start hooks can be chained (content changes, then the Bedrock port check).
- Provider requests are retried on transient failures (connection errors, 5xx, 429).
- Job progress events carry the job kind, so every job shows a meaningful toast.

### Fixed

- The console no longer says "Automatic restart is not enabled" after every crash.
- An automatic restart no longer gives up when a backup or queued plugin changes hold
  the server at that moment; it waits for them.
- Minecraft 26.x player joins and leaves (logged as `System chat: …`) are detected.

- The database is closed on quit (checkpointing the WAL) and when an open is refused
  (for example a schema from a newer MCPanel), so the file is released.
- Integration tests release their databases and verify their temporary directories
  are deleted, instead of silently leaving them in `%TEMP%` on Windows.

### Security

- The content security policy allows images from `cdn.modrinth.com` and
  `hangarcdn.papermc.io` (project icons) only; external links are limited to exact
  allow-listed URLs and the Modrinth/Hangar sites.

- No network listener, no telemetry, strict CSP, argv-only process spawning, redacted
  logs, sensitive-file guard.
