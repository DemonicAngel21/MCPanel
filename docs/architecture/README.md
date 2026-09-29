# MCPanel Architecture & Product Specification

Status: **Approved** (2026-09-28). This document is the source of truth for MCPanel's
architecture. Changes to it are made deliberately, via PR, and — for significant
decisions — accompanied by an ADR in [`docs/adr`](../adr).

Legend used throughout:

- **[CONFIRMED]** — from requirements or stable technical fact.
- **[ASSUMPTION]** — an assumption that may be revisited.
- **[VERIFY]** — must be checked against current official documentation before the
  dependent feature is implemented. Verification results are recorded in
  [`verification-log.md`](verification-log.md).

---

## 1. Product

MCPanel is a Windows-first desktop control panel for **local** Minecraft Java servers.
Primary user: someone hosting for friends/a small community on their own PC; secondary:
power users running several servers (modded, test, SMP).

Principles: safe by default · honest (never fake a metric/capability) · fast (huge
console output must not freeze the UI) · no lock-in (servers remain ordinary folders) ·
local first (no accounts, no cloud service, **no telemetry**).

Non-goals for v1: hosting in an MCPanel cloud, Kubernetes, Docker, Pterodactyl
infrastructure, marketplaces, billing, MCPanel user accounts, mandatory remote management.

## 2. Technology stack

| Layer | Choice |
|---|---|
| Shell | Tauri 2 (WebView2) — see [ADR-0002](../adr/0002-tauri-react-rust-stack.md) |
| UI | React, TypeScript (strict), Vite, TanStack Router/Query/Virtual, Zustand, Tailwind + Radix primitives (shadcn-style components), Monaco (bundled locally, no CDN), uPlot |
| Core | Rust stable, tokio, serde, thiserror, tracing |
| DB | SQLite (WAL) via SQLx, embedded forward-only migrations |
| HTTP | reqwest with rustls |
| System | sysinfo, `windows` crate (Job Objects, registry), notify, keyring/DPAPI |
| TS bindings | ts-rs (generated from API DTOs) — see [ADR-0006](../adr/0006-ts-rs-bindings.md) |
| Package manager | pnpm (version pinned in `packageManager`) |
| Installer | Tauri bundler, **per-user NSIS** (no admin) |

## 3. Layering (most important principle)

```
 React UI  (view state only; no business logic)
    │  typed IPC commands + Channels (streams) + events
 Transport adapter  (apps/desktop/src-tauri — v1: Tauri IPC; future: HTTP/WS)
    │
 Application API  (crates/mcpanel-api — DTOs, validation, Principal/authz, error codes)
    │
 MCPanel Core  (crates/mcpanel-core — domain, services, ports/traits, events, jobs)
    │
 ┌──┴─────────────┬───────────────────┬────────────────────┬──────────────────┐
 mcpanel-db      mcpanel-providers    mcpanel-platform     secret store
 (SQLite repos)  (Mojang, Paper,      (Windows: processes, (Credential Mgr /
                  Purpur, downloads)   Java discovery,      DPAPI)
                                       disks, ports)
```

Dependency rule: UI → API types; API → core; core → only its own traits; adapters →
core traits. The core never exposes Tauri, SQLx, reqwest or Win32 types.

## 4. Core modules

`server` (aggregate, registry, create/import/delete, OperationLock) · `lifecycle`
(supervisor, state machine, readiness/exit classification, restart policy) · `console`
(line pipeline, dialects, ring buffer, capture files) · `jobs` · `events` · `policies`
(crash, disk, tunnel) · `catalog` (Minecraft versions, ordering) · `java` · `files`
(SafePath, file ops, archives) · `config` (lossless `.properties`, schema registry,
targeted YAML/TOML patching) · `content` · `players` · `backup` · `crypto` · `cloud` ·
`network` · `monitoring` · `search` · `templates` · `audit` · `notify` · `settings`.

## 5. Server lifecycle

Two orthogonal dimensions per server:

- `LifecycleState`: `Created → Starting → Running → Stopping → Stopped`, plus
  `Crashed`, `Restarting`, `Error`, and `Detached` (orphan from a previous MCPanel run).
- `ActiveOperations`: set of `{BackingUp, Updating, InstallingContent, Restoring, …}`
  each linked to a Job.

`OperationLock` defines a compatibility matrix (e.g. Restore requires Stopped + exclusive;
live backup is compatible with Running but not with Updating).

**Pending changes:** on Windows the JVM locks its JARs, so content changes on a running
server are queued and applied on next stop or on "Apply & Restart". [CONFIRMED]

### Process supervision

1. Preflight: validated Java, directory exists, EULA accepted, ports free, disk space,
   no conflicting operation.
2. Spawn `java` directly (never through a shell or `run.bat`), `CREATE_NO_WINDOW`,
   piped stdio, explicit minimal environment, assigned to a **Job Object without
   KILL_ON_JOB_CLOSE** (so servers survive an MCPanel crash; the job is used to terminate
   the process tree on force-stop).
3. stdout/stderr → console pipeline (lossy UTF-8, never blocks the child).
4. Readiness via dialect regex (`Done (…s)!`); startup timeout only **warns**.
   Known fatal patterns (port bind failure, EULA, `UnsupportedClassVersionError`) →
   `Error` with diagnosis.
5. Stop: send the software's stop command, wait grace period (default 60 s), then
   terminate the job (recorded as forced).
6. Exit classification: intentional if MCPanel issued stop, the user typed `stop`, or the
   "Stopping the server" line was seen; otherwise crash.

### Quit behaviour and orphans (decided)

- Closing the window minimises to the system tray (one-time explanatory toast).
- Quitting with running servers asks: **Stop servers gracefully and quit** (default) /
  **Quit and leave servers running** / **Cancel**.
- `server_runtime_state` persists `pid` + `process_start_time`. On launch, orphans are
  detected (PID **and** start time must match, preventing PID-reuse mistakes) and shown as
  **Detached**: console input unavailable, OS metrics available. User chooses **Wait for
  exit** or **Force stop** (with an unsaved-world-data warning).

### Crash handling & restart policy

Capture exit code + console tail + new `crash-reports/` file; classify (OOM, wrong Java,
port bind, watchdog, mod/plugin error, unknown); persist; optional crash backup; restart
with `max_attempts` within `window` (default 3 in 10 min), delay 10 s with backoff,
counter reset after 5 min stable uptime. Unfixable classifications (wrong Java, EULA,
port conflict) never auto-restart.
Implemented in v0.2 as a policy listening to `ServerCrashed` (tables
`server_restart_policies`, `crash_events`); auto-restart is on by default, waits for
operations that briefly hold the server, and does nothing if the server was started or
removed during the delay. A process killed from outside MCPanel counts as a crash.

## 6. ServerProvider architecture

Capability-segmented traits, registered in a `ProviderRegistry`:

- `ServerSoftware` — id + `SoftwareCaps` (content ecosystems, datapacks, `is_proxy`,
  log dialect, stop command, TPS source, Geyser variant, `requires_build_step`,
  `eula_required`).
- `SoftwareCatalog` — game versions and builds (stable/experimental channels).
- `SoftwareInstaller` — returns an `InstallPlan` (data: Download{url, hash}, RunInstaller,
  WriteFile, Verify) executed by the core's generic `PlanExecutor`.
- `LaunchResolver` — returns a `LaunchSpec` (java, JVM args, argfiles, jar/main class,
  server args, cwd).
- `SoftwareDetector` — identifies software in an existing directory (import).

Providers return **plans and specs as data**; the core owns execution. Minecraft versions
are opaque ids ordered by manifest release time (never parsed as semver).

Provider roadmap: Vanilla, Paper, Purpur (MVP) → Fabric (v0.2) → Forge, NeoForge, Quilt
(v0.3) → Velocity/BungeeCord, Folia (later). **Spigot/CraftBukkit via BuildTools: not v1**
(architecture keeps `requires_build_step`).

## 7. Content (plugins/mods/datapacks)

One `ContentProvider` trait (search, project, versions, identify-by-hash, trust level);
separate UIs and compatibility rules for plugins and mods. Install pipeline: resolve →
plan (dependency graph, loader/MC compatibility, server-side check) → user confirms →
download to staging (HTTPS) → verify hash (SHA-512 preferred; missing hash flagged
"unverified") → validate archive descriptor → atomic move (old file to trash) → record →
rollback on failure. Disable = move to `<server>/.mcpanel/disabled/`.

Providers: Modrinth, Hangar (v0.2); Spiget for SpigotMC (limited, labelled);
**CurseForge later** (after API-key/terms verification; not blocking MVP).

## 8. Backups

Pipeline: request → OperationLock → consistency strategy → file selection → archive →
[encrypt] → destinations → manifest → verify → retention → event.

- **Local format v1: plain ZIP (deflate)** with `mcpanel-manifest.json`; openable in
  Explorer. Other formats (tar.zst, chunked incremental repository) later via the
  `BackupFormat` trait.
- Consistency: running server → `save-off` / `save-all flush` / wait for confirmation /
  archive / `save-on` (guaranteed in a finally path); pre-update and pre-restore backups
  stop the server.
- Retention: GFS per destination, plus an optional size cap (GiB) for scheduled backups; manual and pre-restore backups never auto-deleted
  unless configured.
- Restore: stop → pre-restore backup → staged extraction with archive safety →
  manifest verification → diff (highlighting changed JARs) → atomic swap.
- Implemented in v0.2: default folder `%USERPROFILE%\MCPanel\Backups` (configurable,
  must not overlap a server folder), one folder per server, file names in local time;
  excluded: `.mcpanel/`, `session.lock`, links; files locked by another program are
  skipped and listed in the manifest. Schedule + retention are stored together in
  `backup_policies` while only the local destination exists (destinations arrive with
  cloud backups in v0.4). A backup blocks server starts/restarts until it finishes.

### Sensitive files (decided)

Files have a sensitivity class: `Normal` or `HighlySensitive` (data-driven patterns,
e.g. `plugins/floodgate/key.pem` and loader equivalents). Highly sensitive files are:

- **included in local backups** (manifest flag `contains_sensitive: true`, UI badge,
  warning before copying a backup elsewhere);
- **always excluded from cloud backups** (not overridable);
- hidden from normal browsing (locked placeholder), blocked from editor, preview, search,
  export, diagnostics bundles, logs, events and API responses;
- accessible only via an explicit, audited "Reveal / Export sensitive file" action.

## 9. Encryption & cloud

- Format: **age** (X25519 recipient; STREAM ChaCha20-Poly1305). No custom crypto.
- Backup Master Key (age identity) stored locally via DPAPI / Credential Manager.
- Recovery Kit (identity wrapped with passphrase via age scrypt) — export is required
  during encryption setup.
- **Cloud copy of the passphrase-wrapped key: opt-in, off by default**, with a clear
  explanation of the trade-off.
- Implemented in v0.4 for local backups: global "encrypt new backups" switch, `.zip.age`
  files, `backups.encrypted` column; the Recovery Kit is an armored scrypt age file with
  a short plain-text header. Decrypted copies for verify/restore are written next to the
  archive as `.mcpanel-plain-*` and removed afterwards (and on the next start after a
  crash). The manifest HMAC below arrives with cloud destinations.
- Manifests carry an HMAC (key derived from the identity via HKDF) to defeat forged
  backups; verified before restore.
- **Cloud backups are encrypted by default.** Disabling encryption is an explicit
  per-destination opt-out with typed confirmation, a persistent "Unencrypted" warning,
  and an audit record.
- Providers: OneDrive (Graph), Dropbox (API v2), Google Drive (API v3) behind
  `CloudStorageProvider`; OAuth 2 PKCE + loopback redirect using **MCPanel's own app
  registrations** (public clients, no secrets). App-verification requirements per
  provider are [VERIFY] before v0.4.
- Implemented (connection foundation): `cloud::CloudStorageProvider` (exchange, refresh,
  account, revoke) with Google Drive / OneDrive / Dropbox adapters; `CloudService` binds
  the loopback listener, builds the PKCE authorization URL, checks `state`, stores the
  refresh token in the `SecretStore` and the account name in settings; access tokens are
  cached in memory and refreshed a minute before expiry. Client IDs come from
  `MCPANEL_GOOGLE_CLIENT_ID`, `MCPANEL_MICROSOFT_CLIENT_ID`, `MCPANEL_DROPBOX_CLIENT_ID`
  (runtime environment, else baked in at build time); a provider without one shows as
  "not configured".
- **Exception (decided 2026-09-30): Google's Desktop client secret.** Google's token
  endpoint requires `client_secret` for "Desktop app" clients even with PKCE; Google
  documents it as embedded in installed apps and "obviously not treated as a secret".
  Releases embed it from `MCPANEL_GOOGLE_CLIENT_SECRET` (a CI secret, never in Git); it
  is held in memory by the Google adapter, sent only on the code exchange and refresh,
  redacted from logs and absent from errors, diagnostics and the UI. It is treated as
  non-confidential configuration, not as a credential MCPanel protects. PKCE S256 is
  unchanged; OneDrive and Dropbox remain secret-less public clients. Uploading backups is the next step. Verified requirements:
  verification-log.md, "Cloud storage OAuth".

## 10. Networking

- `TunnelProvider` trait with `TunnelCaps.can_create_via_api`.
- **Playit:** supervise the official agent; use only officially supported interfaces.
  If no supported tunnel-creation API exists, fall back to agent control + claim flow +
  dashboard deep-link. **No undocumented/private APIs; never claim automation that
  isn't there.** [VERIFY in a dedicated spike before v0.3]
- Notifications (`notify`): a policy turns events (crash recorded, job failed, player
  joined) into drafts; per-category rules (`inbox`, `desktop`) decide whether a draft
  is stored in the `notifications` table (newest 500 kept) and/or announced to the host
  as `NotificationCreated`, which shows the Windows notification.
- Bedrock: `BedrockSetupPlanner` (data-driven table per software/topology), Geyser +
  Floodgate installation, targeted config patching, RakNet unconnected-ping probe.

## 11. Java runtimes

Discovery (JAVA_HOME, PATH, registry vendor keys, common directories, Minecraft Launcher
runtimes, MCPanel-managed), validation by executing `java -XshowSettings:properties
-version`, per-server selection, compatibility from Mojang version JSON
`javaVersion.majorVersion` (vanilla) and data tables. 32-bit Java flagged.

## 12. Data

SQLite at `%LOCALAPPDATA%\MCPanel\mcpanel.db` (WAL, foreign keys). **Minecraft files are
the source of truth for Minecraft config** (server.properties, ops/whitelist/bans); the
DB stores MCPanel's own data. Migrations are forward-only; a `VACUUM INTO` copy is taken
before migrating; a DB with a newer schema than the app knows is refused.

Entities: servers, server_launch_configs, server_restart_policies, server_runtime_state,
java_runtimes, players, server_players, player_sessions, installed_content,
content_dependencies, pending_changes, backup_policies, backup_schedules,
backup_destinations, backups, backup_artifacts, storage_accounts, encryption_keys,
tunnel_accounts, tunnels, crash_events, jobs, audit_events, notification_channels,
notification_rules, notifications, templates, api_tokens, app_settings. Tables are added
by the migration of the phase that introduces them.

## 13. Application API

Transport-agnostic commands/queries/subscriptions with REST-like naming
(`servers.start` ↔ future `POST /api/v1/servers/{id}/start`). Stable error codes
(`ApiError { code, message, details, retryable }`). Every call carries a `Principal`
(v1: `LocalUser`). Streams use sequence numbers for resumption. **No HTTP listener in
v1**, not even loopback; a future local API is opt-in, 127.0.0.1-only, token-auth with
Host/Origin checks.

## 14. Events

Typed `DomainEvent` enum in an envelope (id, time, server, correlation). tokio broadcast
fan-out; slow subscribers lag rather than block. Events never carry secrets. Multi-step
reactions are **policies that run a single Job**, not event chains.

## 15. Files

`SafePath` is the only path type file operations accept: built from (server id, relative
path); rejects absolute/UNC/drive paths, `..`, NUL, `:` (ADS), reserved device names
(incl. with extensions), trailing dots/spaces, control characters, overlong components;
case-insensitive prefix checks. Reparse points are never traversed for write/delete/
move/extract. Imports come only from native dialogs (one-time grants). Archive extraction
enforces zip-slip checks, symlink rejection, entry/size/ratio limits, collision checks,
staging. Atomic writes (temp + flush + replace), BOM and line endings preserved.

Default servers folder: **`%USERPROFILE%\MCPanel\Servers`** (never Documents/Desktop,
which are often OneDrive-redirected). Warnings for OneDrive-synced, network or
Program Files locations.

## 16. Console & performance

Reader task → lazy parse → per-server ring buffer (default 20k lines) → buffered capture
file → batched UI delivery (≤50 ms / ≤500 lines) with sequence numbers and `Gap` markers
under backpressure. The child's stdout is never blocked. UI renders with virtualization;
search runs in the core. Minecraft `§` codes and ANSI parsed to spans; never raw HTML.

Metrics are labelled by source: **OS** (sysinfo), **JVM** (opt-in, later), **Minecraft**
(Paper `tps`/`mspt`, vanilla `/tick query` [VERIFY]), **Plugin-provided** (spark). A
metric that cannot be obtained is shown as "Not available", never estimated.

## 17. Security (threat model summary)

Assets: host filesystem, cloud tokens, backup keys, Playit secret, Floodgate key, API
tokens, world data. Trust boundaries: UI↔core, core↔server files (**untrusted**),
core↔internet, core↔child processes (**untrusted code**).

Key mitigations: SafePath; no shell execution and argv-only process spawning; strict CSP
and sanitised rendering; HTTPS + hashes for downloads; secrets in OS stores with secret
types that cannot be logged/serialised; manifest HMAC and staged restore against backup
poisoning; imported server directories treated as untrusted data (never execute their
scripts); per-user install, never elevated; RCON off by default; no telemetry.

## 18. Secrets

`SecretStore` port (Credential Manager; DPAPI blobs for larger items). DB stores only
references. In-memory `secrecy` types, zeroised, not `Debug`/`Serialize`. Secrets are
write-only from the UI's perspective. Redacting log writer as defence in depth.

## 19. Logging

`tracing` with per-domain files in `%LOCALAPPDATA%\MCPanel\logs\` (`app`, `server`,
`backup`, `content`, `cloud`, `tunnel`, `java`, `api`), daily rotation, 14-day retention.
Console captures in `%LOCALAPPDATA%\MCPanel\servers\<id>\console\`. MCPanel never
modifies a server's own `logs/`.

## 20. UI information architecture

Global rail: **Dashboard · Servers · Backups · Network · Java · Templates · Activity ·
Settings** + notification inbox + Ctrl+K command palette.

Server workspace: **Overview · Console · Files · Players · Content (Plugins/Mods/
Datapacks by capability) · Backups · Network · Performance · Configuration (Properties,
Launch, Restart policy, Software, Danger zone) · Activity**.

Plugins/Mods/Players are server-scoped; globally they appear as an Updates view and via
Ctrl+K. Visual design: dark default + light mode, neutral greys, one restrained green
accent, Inter + JetBrains Mono, 120–200 ms purposeful motion, dense-but-calm tables.

## 21. Updates, release and signing

- `tauri-plugin-updater` with signed metadata on GitHub Releases (later phase).
- Release: feature PRs → release PR (version bump + changelog) → full CI → manual
  installer checklist on a clean VM → tag → release workflow builds → **sign step** →
  draft GitHub Release → human publishes.
- **Code signing (decided):** public releases are signed. The workflow has a dedicated
  signing step behind `scripts/sign.ps1`; without signing secrets builds are labelled
  "unsigned" and cannot be published as stable. Updater signing key is separate.

## 22. Extensibility

Built-in providers + `ProviderRegistry`, each with a shared contract test suite.
Declarative extension points (templates, property schemas, log dialects, Java tables,
Geyser maps) as versioned data files. **No third-party code extensions in v1**; if ever,
out-of-process API clients with scoped tokens or WASM components — never native DLLs.

## 23. Testing

Unit (state machine, policies, SafePath, parsers, planners), property tests (proptest),
fuzzing (archives, parsers — scheduled), process tests against `tools/fake-mc`, nightly
real-server tests, provider fixture tests, migration tests, backup round-trip and tamper
tests, redaction tests, UI component tests (Vitest + RTL), Playwright E2E with mocked
API, console flood performance test.

**Minecraft test matrix (decided):** 1.12.2 (Java 8), 1.16.5 (Java 8/11), 1.20.1
(Java 17), 1.21.x (Java 21), current 26.x (Java per Mojang metadata). Older versions are
**best-effort**.

## 24. Git & CI

`main` stable; `feature/*`, `fix/*`, `refactor/*`, `docs/*`; Conventional Commits;
SemVer tags; Keep-a-Changelog. CI: fmt, clippy (`-D warnings`), tests, ESLint,
TypeScript, Vitest, production build on windows-latest. Security CI: cargo-deny,
pnpm audit, gitleaks, CodeQL, Dependabot, secret scanning + push protection; actions
pinned by SHA with minimal permissions.

Public repository; license **MIT OR Apache-2.0**.

## 25. Roadmap

| Phase | Theme | Contents |
|---|---|---|
| **v0.1 MVP** | Run servers safely | Shell + tray, DB/migrations, events/jobs/audit, Java detection, Vanilla/Paper/Purpur, create wizard (EULA), import, lifecycle + orphan detection, console, files, editor, properties editor, basic metrics, CI |
| **v0.2** | Protect & extend | Local backups/schedules/retention/restore, players, plugin manager (Modrinth, Hangar), crash auto-restart, built-in templates, Fabric |
| **v0.3** | Play together | Mod manager (Modrinth; CurseForge once verified), Forge/NeoForge/Quilt, Playit, Geyser/Floodgate, notifications |
| **v0.4** | Offsite & insight | Encryption, OneDrive/Dropbox/Google Drive, performance monitoring, crash diagnostics, notification rules, disk policy, Spiget |
| **v0.5** | Hardening | Security review, fuzzing, updater, signed installer, docs, accessibility |
| **v1.0** | Stable | Polish, migration guarantees, support matrix |

## 26. Decisions (2026-09-28)

| # | Decision |
|---|---|
| 1 | MIT OR Apache-2.0; public GitHub repository |
| 2 | Close window → tray; quitting with running servers asks, default graceful stop |
| 3 | No kill-on-close; detect orphans on next launch; user chooses wait or force stop |
| 4 | Default servers folder `%USERPROFILE%\MCPanel\Servers` |
| 5 | Plain ZIP local backups for v1 |
| 6 | Floodgate key included in local backups, classified highly sensitive |
| 7 | Cloud-stored wrapped key: supported, opt-in |
| 8 | Unencrypted cloud backups: explicit opt-out only, with warnings |
| 9 | MCPanel's own OAuth app registrations; verify provider requirements first |
| 10 | CurseForge eventually; not blocking MVP |
| 11 | Spigot/BuildTools later, not v1 |
| 12 | Signed public releases; signing step pluggable, development not blocked |
| 13 | No telemetry; any crash reporting opt-in |
| 14 | Test matrix 1.12.2 / 1.16.5 / 1.20.1 / 1.21.x / 26.x; older best-effort |
| 15 | Per-user NSIS installer |
| 16 | Playit fallback accepted; no undocumented/private APIs |
| 17 | Use installed pnpm (12.6.0) |
