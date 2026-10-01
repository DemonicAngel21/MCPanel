# MCPanel handoff

Written 2026-10-01; update this section as work continues. Read this first, then
`README.md`, `docs/development.md`, `docs/architecture/README.md`, the ADRs in `docs/adr/`,
`docs/architecture/verification-log.md` and `docs/polish-backlog.md`.

## Active owner request: major UI/UX refinement

Keep every existing feature and action. Make MCPanel more visual, concise, information
dense, polished, responsive and consistent, like a professional Minecraft server control
panel. Do not remove, hide, disable, simplify or break functionality. Retain important
warnings, security details, recovery guidance, configuration and accessibility. Avoid fake
metrics, decorative gradients, generic dashboard cards, prose-heavy explanations and
unnecessary empty space.

The owner specifically requested:

- Use real data for compact charts: CPU/RAM, disk, TPS/MSPT, players, backups, uptime and
  network only where data exists. Add hover/tooltips, dark/light support, responsive sizing
  and honest empty states. Never invent samples.
- Identify Vanilla, Paper, Purpur, Fabric, Forge, NeoForge, Quilt, Spigot and Bukkit with
  consistent software-specific marks. Use safe existing assets or distinctive clean marks.
- Show actual player heads when available, with graceful fallback. Keep online state,
  operator and whitelist/ban status, ping/playtime/session details when the data exists,
  and all player actions.
- Make the dashboard and server cards quickly communicate state, software/version,
  players, resources, performance, uptime, storage, activity, backups and warnings without
  a wall of cards.
- Use concise copy, consistent typography, icons, cards, controls, tables, badges, tabs,
  dialogs, tooltips, empty/loading/error states and subtle fast animations. Maintain both
  dark and light themes and usable layouts at different window sizes.
- Review every page for clarity within a few seconds. Preserve server create/import,
  lifecycle, console, files/editor, properties, players, content/plugins, backups,
  encryption/restore, cloud/Google Drive/Dropbox, Playit, Geyser/Floodgate, TPS/MSPT,
  diagnostics, notifications, Java, settings, account, onboarding, tray and orphan handling.
- Final pass requested: formatting, TypeScript, frontend tests, Rust checks/tests,
  accessibility, dark/light, empty/loading/error states, window sizes, and feature
  regression review. Fix regressions before handoff.

### Cloud Sync, Guest Mode, Microsoft Auth & Server Linking (Completed)

1. **Guest Mode**:
   - `mcpanel-core` now supports guest accounts (`is_guest`, `enter_guest_mode`).
   - Guest users can manage all local servers without cloud sign-in.
   - Distinctive guest badge and clear upgrade flow in UI.
   - Upgrading from guest to an authenticated account migrates local preferences smoothly without data loss.

2. **Microsoft Account Authentication**:
   - Native desktop OAuth 2.0 PKCE flow on loopback redirect (`127.0.0.1:{port}`).
   - Firebase Identity Provider token exchange (`signInWithIdp` with `providerId: "microsoft.com"`).
   - Configurable Microsoft Client ID and optional secret via OAuth settings modal and environment variables.

3. **Cloud Settings Synchronization**:
   - Versioned (`schema_version: 1`), Firestore-backed sync for non-sensitive preferences (`theme`, `accent`, `console_buffer_lines`, `quit_stop_timeout_secs`, `tick_sampling`).
   - Last-write-wins timestamp conflict resolution.
   - Offline resilience: local settings operate unimpeded; sync is attempted on login and manual trigger.
   - Strict safety: passwords, secrets, backup directories, server files, and auth tokens are never uploaded.

4. **Future Server Linking Architecture (ADR 009)**:
   - Added `mcpanel_core::server_link` domain module with `ServerScope`, `ServerEnrollmentToken`, `ServerCredentialSecret`, `LinkedServerRecord`.
   - Clear boundary: server identity is decoupled from user identity, preventing Microsoft/OAuth token reuse as server credentials.
   - Created `docs/architecture/adr-009-server-linking.md`.

5. **Security & Firestore Rules**:
   - Production-ready `firestore.rules` created by `firestore-rules-author` subagent with default-deny, owner isolation (`request.auth.uid == userId`), and strict field whitelisting.

### Current refinement work (completed and verified locally)

The major UI/UX refinement requested by the owner has been implemented, formatted, and
verified against the full test suite. Changes made:

- `apps/desktop/src/components/software-mark.tsx`: distinctive local SVG marks and theme-adaptive
  styling for Vanilla, Paper, Purpur, Fabric, Forge, NeoForge, Quilt, Spigot, Bukkit.
- `apps/desktop/src/components/player-head.tsx`: Minecraft head image by valid UUID with graceful
  initials fallback and error boundary handling. `apps/desktop/src-tauri/tauri.conf.json` allows
  `mc-heads.net` in CSP `img-src`.
- Dashboard rewritten as compact server cards with actual process CPU/RAM sparklines,
  current TPS, uptime, player counts, system metrics, disk bars, latest backups and recent
  activity. Only existing API data is used; queries for metrics are enabled strictly when
  a process is running (`enabled: hasProcess(...)`), avoiding query fan-out.
- Player rows/lists now include heads where UUIDs exist, visible online/operator/list
  badges, session counts and existing playtime. Tab underline styling fixed on click (item 26).
- Software marks added to server list, server header, create/import selection, command
  palette, overview and backup records.
- Fabric/Quilt loader formatting: "Loader 0.19.5" instead of "build #0.19.5" via
  `formatSoftwareBuild` in `lib/format.ts` (item 31).
- Empty parent directory cleanup on deleting the last backup in `crates/mcpanel-core/src/backup/service.rs` (item 45).
- Truncated header paths with full-path tooltips on server Backups and Plugins tabs (items 28, 29).
- Redundant server ID/name suppressed in server Activity tab (item 42).
- Bedrock MOTD quotes stripped on connection card (item 35).
- Bedrock tunnel port auto-detected from server Geyser settings in Playit tunnel dialog.
- Active server tab auto-scrolled into view on narrow screens (item 43).
- Sparkline/time charts rebuild when theme changes; metric cards collapse when server is stopped.
- Shared surfaces, status animations, navigation, table scrolling and shell spacing refined.
- DOMPurify security override added to `pnpm-workspace.yaml` resolving `pnpm audit --prod`.
- Player console replies filter out concurrent WARN log lines in `crates/mcpanel-core/src/players/service.rs` (item 27).
- Template cards display preview badges for resolved properties in modern versions (item 30).
- Settings Privacy card dynamically derives software providers from registered provider list (item 33).
- Bedrock connection card shows a quick-copy 127.0.0.1:{port} address block matching Java Overview (item 36).
- ServerControls provides graceful termination guidance tooltip and enables Force stop on compact cards (item 37).
- Create wizard clarifies multi-file checksums when base game jar uses SHA-1 and libraries use SHA-512 (item 32).
- Dashboard Servers and Players cards include 38px visual metric bars/summaries matching CPU/RAM sparklines (item 41).

### Verification checklist results

- Prettier: `pnpm --filter @mcpanel/desktop format:check` passed cleanly.
- Frontend checks: `pnpm check` passed cleanly (23 vitest tests pass, eslint 0 warnings, tsc clean).
- Rust formatting: `cargo fmt --all -- --check` passed cleanly.
- Rust clippy: `cargo clippy --workspace --all-targets -- -D warnings` passed with 0 warnings.
- Rust tests: `cargo test --workspace` passed cleanly (all 217 unit and integration tests).
- Fake-MC integration tests: `cargo test --package fake-mc` passed cleanly (all 36 lifecycle, backup,
  crash, performance, player, bedrock, content, and playit agent tests).
- Security & licenses: `cargo deny check` passed (advisories ok, bans ok, licenses ok, sources ok).
- Dependency audit: `pnpm audit --prod` passed (0 vulnerabilities).


## What MCPanel is

A Windows desktop control panel for local Minecraft Java servers (Tauri 2 + React 19 +
TypeScript + Tailwind v4, Rust workspace). Create/import servers, start/stop, console,
files and Monaco editor, players, plugins/mods (Modrinth, Hangar, Spiget), Bedrock
crossplay (Geyser/Floodgate), backups (local, optional encryption), crash recovery,
performance graphs, cloud storage account linking (Google Drive, Dropbox), playit.gg
tunnels, Firebase accounts, first-time setup.

- `crates/mcpanel-core`: domain logic and ports (no HTTP). Key modules: `server/`
  (lifecycle), `tunnels.rs` (installed playit service), `playit_agent.rs` (MCPanel's own
  playit agent), `playit_api.rs` (port), `account.rs` (Firebase accounts), `cloud/`
  (OAuth, PKCE, loopback), `settings.rs`.
- `crates/mcpanel-providers`: HTTP adapters (`playit.rs`, `firebase.rs`, `cloud/`,
  software/content providers). Production HTTP is HTTPS-only.
- `crates/mcpanel-api`: permission-checked API + DTOs (ts-rs generates
  `apps/desktop/src/bindings`; run `pnpm gen:bindings` after DTO changes).
- `crates/mcpanel-platform`: Windows specifics (processes/job objects, Credential
  Manager, registry accent color).
- `apps/desktop/src-tauri`: host (commands, tray, startup/exit hooks in `main.rs`).
- `apps/desktop/src`: UI. Pages in `pages/`, shared UI in `components/ui/`.
- `tools/fake-mc`: fake Minecraft server + JDK used by integration tests (`tests/`).

## Rules the owner set (keep following them)

- Git: work on `main`, Conventional Commits, end commits with the co-author line used in
  history. **Never push** unless the owner explicitly asks. No force-push, no public repo,
  no releases. Everything after `origin/main` is **unpushed** (see `git log origin/main..HEAD`).
- License: MIT, "Copyright © 2026 DemonicAngel21". Do not change it again. Do not modify
  third-party licenses.
- Secrets: never put client secrets, tokens, keys or credentials in Git, logs,
  diagnostics, UI, error messages or telemetry. Build-time values come from environment
  variables: `MCPANEL_GOOGLE_CLIENT_ID`, `MCPANEL_GOOGLE_CLIENT_SECRET`,
  `MCPANEL_DROPBOX_CLIENT_ID`, `MCPANEL_FIREBASE_API_KEY` (see `docs/development.md`).
  Never ask the owner to paste a secret into chat.
- The Floodgate key is highly sensitive. Do not commit generated server data.
- No fake functionality. Do not claim something works unless it was actually tested.
- OneDrive was removed on purpose (postponed). Do not reintroduce it.
- Ask before destructive operations on the owner's data (servers, worlds, backups).
- Do not read or use the installed playit service's key (`C:\ProgramData\playit_gg\
  playit.toml`); MCPanel uses its own linked agent (ADR-0007 amendment 2). Using that
  file was explicitly refused by the environment's permission system.

## How to work

```
pnpm install
pnpm dev                          # dev app (see isolated data below)
pnpm check                        # eslint + tsc + vitest
cd apps/desktop && pnpm format:check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace            # ~369 tests, needs Windows
cargo deny check && pnpm audit --prod
pnpm build                        # release + NSIS: target\release\bundle\nsis\MCPanel_0.1.0_x64-setup.exe
```

Isolated dev instance (does not touch the owner's installed MCPanel data, credentials,
single-instance or playit pipe): set `MCPANEL_DATA_DIR`, `MCPANEL_SERVERS_DIR`,
`MCPANEL_BACKUPS_DIR`, plus `WEBVIEW2_USER_DATA_FOLDER` (required while the installed app
runs) and `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` to drive the
UI with `node apps/desktop/scripts/cdp.mjs <script.mjs>`. `scripts/a11y.mjs` runs axe on
every page (and every setup step). Emulate `reducedMotion: "reduce"` for screenshots;
hidden windows freeze CSS animations.

## Previous verified baseline

The prior handoff recorded 369 Rust tests, 12 frontend tests, clippy, formatting, deny,
audit and accessibility checks as passing. Those results predate the active refinement
above; they do not verify its current uncommitted code. Before this request, the checkout
was clean at `c3cf421` and matched `origin/main`.

Done in the last sessions (details in `CHANGELOG.md` and `git log`):
- License → MIT, holder DemonicAngel21; OneDrive removed; animations; UI polish.
- Testing Server investigation: two servers on port 25565; fixed MCPanel to refuse a
  second server on the port of a starting/running one; "Overworld settings missing"
  diagnosed as `world_incomplete` and not auto-restarted.
- Server "Manage" tab (power, live tiles, performance graphs) and Overview split.
- Playit.gg section; **Option 2**: MCPanel links its own self-managed playit agent (claim
  flow), stores its key in Credential Manager, runs `playitd` on its own pipe, manages
  tunnels (create/rename/port/enable/disable/delete); installed service start/stop kept.
- Firebase accounts (email/password, verification, reset, Google sign-in) and the
  first-time setup wizard; Settings → Account.
- Full-width layouts, RAM sliders, accent colors (presets / Windows accent / custom),
  category tints.

## Incomplete work (everything still open)

### Needs the owner (cannot be done without them)
1. **Firebase project.** Create it (steps in `docs/development.md` → "MCPanel accounts
   (Firebase)"), enable Email/Password and Google, and provide `MCPANEL_FIREBASE_API_KEY`
   as a build environment variable. Until then accounts show "not available in this
   build". Tested only against local stand-ins plus one live call proving an invalid key
   is reported correctly. Real sign-up, sign-in, verification/reset emails and Google
   sign-in are **untested against Firebase**. Google sign-in also needs the Google OAuth
   Desktop client to be in the Firebase project's Google Cloud project (or safelisted).
2. **Live playit.gg linking.** The claim flow and tunnel management are tested against an
   in-memory API and the real `playitd` (start/stop with a fake key), but linking to a real
   playit.gg account needs the owner's browser approval, so **live create/edit/delete of
   tunnels is untested**. After linking, verify: tunnel creation returns an address,
   rename/port/enable/delete work, other agents' tunnels stay read-only.
3. **Cloud OAuth values for installers.** The owner has not set
   `MCPANEL_GOOGLE_CLIENT_ID`, `MCPANEL_GOOGLE_CLIENT_SECRET`, `MCPANEL_DROPBOX_CLIENT_ID`
   (and now `MCPANEL_FIREBASE_API_KEY`) where the build can read them (User environment
   variables work: read them into the build process without printing them). Installers
   built so far show Google Drive/Dropbox as "Not configured".
4. **Testing Server repair.** Its `world` folder (`C:\Users\getog\MCPanel\Servers\
   testing-server\world`) was half-created by a failed first start; rename or delete it so
   a new world generates (it contains no chunks). Not done: owner's data. Also give server
   "1" (imported SquidServers world) or Testing Server a different port.
5. **Pushing changes** only when the owner asks. Never push to origin/main unless requested.

### Not yet built / follow-ups
6. **Fresh production installer + end-user test** of everything since the last installer
   (Manage tab, Playit section and agent, accounts, setup wizard, accent colors, sliders):
   build NSIS, install per-user, test as an end user (launch without dev env vars, setup
   wizard, server create/start/stop/console/backup, playit, no orphan Java/playitd after
   quit, uninstall/reinstall keeps data). Last installer tested predates these features.
7. **playitd missing:** MCPanel requires the installed playit program for `playitd.exe`;
   it does not download it. Option: download the signed release asset from GitHub
   (`playit-windows-x86_64-signed.exe` is playitd) with SHA-256 verification.
8. **Playit details:** Bedrock tunnel port defaults to 19132 instead of reading the
   server's Geyser port (`useBedrock(serverId).settings`); only Minecraft Java/Bedrock
   tunnel types; region is always "global" (premium regions not offered); the manual
   "public address" per server (old feature, `tunnel_set_server_address`) still exists as a
   fallback and could be removed or merged; the old service linking via `playit setup`
   (`tunnel_link`) is unused by the UI now.
9. **Accounts have no purpose yet:** signing in only identifies the user. Nothing is
   synced or gated. Decide with the owner what accounts should unlock (e.g. settings sync,
   cloud backups).
10. **Cloud backups upload** is not implemented (accounts can be linked, uploads cannot).
11. **UI refinement** is active; see “Active owner request” above for scope and next steps.
12. **Polish backlog:** see `docs/polish-backlog.md` (e.g. stop during first-run patching
    waits the full timeout, install dir equals data dir, empty backup folder left after
    deleting the last backup, active tab not scrolled into view at narrow widths).
13. **Docs:** user docs now cover accounts (`docs/user/accounts.md`); add a guide for
    the setup wizard, and update the verification log after Firebase and playit are
    tested live.
