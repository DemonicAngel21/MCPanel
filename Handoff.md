# MCPanel handoff

Written 2026-09-30 for the next developer (human or AI). Read this first, then
`README.md`, `docs/development.md`, `docs/architecture/README.md`, the ADRs in `docs/adr/`,
`docs/architecture/verification-log.md` and `docs/polish-backlog.md`.

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

## State at handoff

All checks above pass (369 Rust tests, 12 frontend tests, clippy, fmt, prettier, deny,
audit, accessibility scan with no violations in light and dark). The working tree is
committed.

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
5. **Pushing** the local commits (`git log origin/main..HEAD`), only when the owner asks.

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
11. **"More color" / full-width pass** was applied to the main pages; review remaining
    screens (dialogs, file explorer, content search) for consistency.
12. **Polish backlog:** see `docs/polish-backlog.md` (e.g. stop during first-run patching
    waits the full timeout, install dir equals data dir, empty backup folder left after
    deleting the last backup, active tab not scrolled into view at narrow widths).
13. **Docs:** user docs cover playit and troubleshooting; add user docs for accounts and
    the setup wizard, and a verification-log entry once Firebase and playit are tested
    live.
