# Changelog

All notable changes to MCPanel are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Quality of Life enhancements across the server workspace:
  - **File Editor**: Global autosave synchronization and Word Wrap toggle persisted across all files and folders.
  - **Server Console**: Quick command action bar (`tps`, `mspt`, `list`, `save-all`, `day`, `clear weather`, `reload`) for instant one-click execution, live line count feedback, and quick clear search action.
  - **File Manager**: Category filter pills (`All`, `Configs`, `Logs`, `Jars & Zips`, `Folders`) with item counts, quick search reset button, and one-click breadcrumb folder path copying.
  - **Player Manager**: Instant player search filter across all player categories (Known Players, Whitelist, Operators, and Bans) with filtered counts.
  - **Server Overview**: One-click server directory folder path copying.
- Cracked / Offline-mode server setting: dedicated `OnlineModeCard` in Server Settings with toggle switch for `online-mode=false` and advisory warning banners explaining that unauthenticated/cracked clients are allowed. Also integrated cracked-mode awareness into server creation and properties synchronization without disrupting Geyser/Floodgate or other configurations.
- Recommended plugins system: extensible, contextual recommendation engine in the core layer that surfaces plugins based on server software (Paper, Purpur, Spigot, Fabric, etc.), Minecraft version, offline-mode security (AuthMe, FastLogin, SkinsRestorer), and Bedrock crossplay (Geyser, Floodgate) across 10 categories, complete with category filtering and dependency-verified direct installation.
- Automatic imported plugin detection: heuristic detection engine that inspects descriptor metadata (`plugin.yml`, `fabric.mod.json`, `quilt.mod.json`, `mods.toml`), main classes, and filename patterns to automatically identify imported jars with confidence levels (`Exact`, `High`, `Medium`, `Low`, `Unknown`). Includes an "Identify / Link" dialog allowing users to connect or correct plugin providers and cache results.
- Microsoft account authentication: desktop OAuth 2.0 PKCE flow with loopback redirect,
  runtime Azure client configuration, and Firebase identity provider sign-in.
- Guest account mode: allows full local server-management without an account, with an inline
  upgrade path that migrates local preferences upon sign-in.
- Cloud preferences synchronization: Firestore-backed cloud synchronization for approved
  non-sensitive settings (theme, accent, console buffer lines, quit timeout, tick sampling)
  with schema versioning, last-write-wins timestamp conflict resolution, and offline resilience.
- Server linking architecture (ADR 009): account identity domain model supporting future
  MCPanel Minecraft server plugin enrollment with revocable secrets, server-scoped tokens,
  and decoupled server identities.
- Hardened Firestore security rules (`firestore.rules`) enforcing default-deny, owner-only
  access boundaries, and strict field whitelisting.
- Runtime Google OAuth configuration: enter and securely persist Google OAuth Client ID,
  client secret, and Firebase API keys dynamically at runtime, enabling "Continue with Google"
  without rebuilding or setting environment variables.
- Multihost cluster management: monitor and manage Minecraft servers across multiple
  host nodes (local machine and remote nodes). Remote node enrollment and management
  requires an active MCPanel account (Firebase Authentication). Unauthenticated users
  receive an account prompt while retaining local host visibility; authenticated users can
  enroll remote nodes, test latency with ping checks, generate token pairing commands,
  and inspect hardware telemetry.
- First-time setup (account, appearance, Java, playit.gg) and optional MCPanel accounts
  with Firebase Authentication: email and password (verification and reset emails) or
  Continue with Google.
- First-time setup and the Playit.gg page offer a direct link to the official Playit
  agent installer when it is missing; setup now includes a progress indicator and
  restrained step and card transitions.
- playit.gg: MCPanel links its own agent to your playit.gg account, runs it, and creates,
  edits, enables/disables and deletes tunnels for your servers.
- Accent colors (presets, Windows accent or custom), RAM sliders, full-width layouts and
  category colors.
- Servers open on a new Manage tab: power controls, live CPU, memory, players, TPS and
  MSPT, and performance graphs (5/15/30 minutes) with axes and hover values.
- Playit.gg has its own section: the agent (start, stop, link) and each server's local
  and public address.
- playit.gg: start and stop the playit agent, link it to a playit.gg account through
  the official `playit setup` flow, and save a tunnel's public address per server.
  Tunnels are still created on playit.gg (no public API).

### Fixed

- Whitelist and offline-mode player management bug: fixed source-of-truth discrepancy where the Players tab instructed users to enable the whitelist even when it was already enabled in offline mode. The tab now accurately distinguishes between offline mode with whitelist active (info banner) and offline mode with whitelist disabled (warning banner). Added instant disk synchronization when toggling whitelist state and comprehensive Vitest regression tests.
- Microsoft sign-in auto-polling: fixed `refetchInterval` in account status query to poll
  when `microsoftState === "waiting"`, automatically reflecting sign-in upon browser redirect.
- Onboarding guest mode handling: fixed `AccountStep` in setup wizard to recognize guest users
  and render `AccountSummary` instead of repeating sign-in forms.
- NumberSetting synchronization: fixed numeric input controls in Settings to update their internal
  text state when canonical values change via cloud sync or background reloads.
- Verification badges on OAuth accounts: scoped email verification badges and resend actions
  strictly to email/password accounts, removing misleading "Not verified" tags on Google/Microsoft profiles.
- OAuth configuration modal isolation: refactored dialog to mount fresh form state on open, preventing
  credential leakage across dialog sessions.
- Cloud storage provider name closure: resolved stale closure in polling timer using synchronized ref.
- Fixed off-centered online status dot on host machine badges by using the standardized
  StatusDot component.
- Multihost logout state synchronization: bound multihost authenticated view to `account.signedIn`
  and immediately reset multihost query cache on sign-out, eliminating stale signed-in states
  without requiring a manual page refresh.
- Temporarily blocked Microsoft authentication across core and desktop UI until Azure testing
  credentials are ready, preserving all underlying OAuth/PKCE implementation code intact.

### Changed

- Synced editor autosave preference: elevated editor autosave from an isolated per-file toggle to a synchronized application-wide setting persisted in local storage. Toggling autosave in any file or folder now instantly applies across all files, folders, and servers without requiring manual activation per file.
- Enhanced CSP configuration in `tauri.conf.json` to permit Google user avatar CDN domains (`lh3.googleusercontent.com`).
- Refined Dashboard Players stat card to display active player head stacks and symmetric session status.
- Refined Servers table row hierarchy with flush software-mark alignment for server paths.
- Added responsive table scrolling (`table-scroll`) to Java runtimes table for narrow screens.
- Added visual checkmark and tooltip confirmation on Server Overview connection address copy action.
- Added pagination spinner feedback to Activity audit log loading button.
- Added commercial desktop release badge and refined build metadata in Settings About panel.

- Playit agent linking reports the installed daemon version; tunnel requests use the
  current v1 field names and keep local address settings in the agent config.
- Playit linking now offers recovery for an invalid saved agent key and exposes the
  approval link if the browser cannot open it.

- A server cannot start while another MCPanel server on the same port is starting or
  running; a half-created world ("Overworld settings missing") is diagnosed and not
  auto-restarted.

- OneDrive is no longer offered as cloud storage (postponed); Google Drive and Dropbox
  remain supported.
- MCPanel is now licensed under the MIT License only (previously MIT OR Apache-2.0).
  Copyright © 2026 DemonicAngel21. Dependency licenses are unchanged.

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
- Google Drive sign-in sends Google's Desktop client secret (embedded in release builds
  from `MCPANEL_GOOGLE_CLIENT_SECRET`), because Google's token endpoint requires it for
  Desktop clients even with PKCE. Google treats this value as non-confidential; MCPanel
  keeps it out of Git, logs, diagnostics, errors and the UI.
- Cloud storage connections (v0.4 foundation): Settings can connect and disconnect
  Google Drive, OneDrive and Dropbox with OAuth 2.0 + PKCE through the browser and a
  loopback redirect (no client secret). Refresh tokens are kept in the Windows Credential
  Manager and refreshed automatically; disconnect revokes access where the provider
  supports it. Providers without an app registration show as not configured. Uploading
  backups is not available yet.
- Disk policy (v0.4): a notification when a drive with servers or backups drops below
  5 GB or 5 % free (once per drive until it recovers); an optional size limit for a
  server's scheduled backups (oldest removed first, newest always kept); a disk-usage
  breakdown per server (worlds, plugins/mods, logs, other, backups, free space).
- Crash diagnostics (v0.4): each crash records the root-cause exception and the
  plugins/mods that ran in the crashing code (from the jar named in stack frames, or
  from class packages unique to one installed jar); the crash history shows them. A
  "Diagnostics" button saves a support ZIP (logs, crash reports, console captures,
  redacted server.properties, plugin/mod list) without worlds or sensitive files.
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

### Security

- Launch settings: JVM arguments are allowlisted (no `-XX:OnOutOfMemoryError`,
  `-javaagent`, `@argfiles`, `-jar`/`-cp`, code-loading system properties) and server
  arguments cannot contain paths.
- Key files (Floodgate key, Geyser tokens) can no longer be renamed, moved or copied
  out of their protection; secrets in `server.properties` are hidden in the text editor
  and redacted completely in diagnostics bundles (including the 1.21.9
  management-server secret and keystore password).
- MCPanel never creates folders or moves files through a linked `.mcpanel` folder or
  other junctions (trash, unzip, restore, Bedrock config, diagnostics).
- Downloads without a published hash are not redirected; provider responses are size
  capped while streaming.

### Fixed

- Adding a key to a `server.properties` that ended with a line-continuation backslash
  merged the new key into the previous value (found by property-based tests).
- Accessibility: labels, names, landmarks, heading order and 4.5:1 text contrast in both
  themes (axe-core finds no violations on the main pages).

- Paper watchdog shutdowns (exit code 70, "Stopping server" in the log) were recorded
  as normal stops, so no crash was handled and auto-restart never ran.

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
