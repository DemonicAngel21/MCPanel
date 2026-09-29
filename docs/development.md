# Development guide

## Prerequisites (Windows 10/11 x64)

| Tool | Version used | Notes |
|---|---|---|
| Rust | 1.98.1 (pinned in `rust-toolchain.toml`) | MSVC toolchain |
| Visual Studio Build Tools | 2026, C++ workload + Windows SDK | required by Rust/Tauri |
| Node.js | ≥ 24 (26.7 used during development) | |
| pnpm | 12.6.0 (pinned via `packageManager`) | |
| WebView2 Runtime | preinstalled on Windows 11 | |
| Java | any 64-bit runtime | only needed to run real servers |

TypeScript is pinned to 6.0.x because `typescript-eslint` does not yet support
TypeScript 7.

## Everyday commands

```powershell
pnpm install
pnpm dev                     # Tauri dev (Vite on 127.0.0.1:1420 + Rust host)
cargo test --workspace       # all Rust tests incl. fake-mc lifecycle tests
pnpm check                   # lint + typecheck + Vitest
pnpm --filter @mcpanel/desktop format    # Prettier
pnpm gen:bindings            # regenerate TS bindings after changing API DTOs
pnpm build                   # production build + per-user NSIS installer
cargo deny check             # advisories / licenses / sources
```

## Isolated data directories

`MCPANEL_DATA_DIR`, `MCPANEL_SERVERS_DIR` and `MCPANEL_BACKUPS_DIR` override
`%LOCALAPPDATA%\MCPanel`, `%USERPROFILE%\MCPanel\Servers` and
`%USERPROFILE%\MCPanel\Backups`. Set all three so development never touches your real
data.

## UI end-to-end runs with fake-mc

`tools/fake-mc` impersonates `java.exe` running a Minecraft server (no downloads, no
EULA). `mcpanel-seed` prepares an isolated data directory with one fake server:

```powershell
cargo build -p fake-mc
$e2e = "$env:TEMP\mcpanel-e2e"
.\target\debug\mcpanel-seed.exe "$e2e\data" "$e2e\servers" .\target\debug\fake-mc.exe
$env:MCPANEL_DATA_DIR = "$e2e\data"; $env:MCPANEL_SERVERS_DIR = "$e2e\servers"
$env:MCPANEL_BACKUPS_DIR = "$e2e\backups"
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
pnpm dev
# In another terminal: drive the WebView over CDP (Playwright)
node apps/desktop/scripts/cdp.mjs my-flow.mjs
```

Note: `pnpm dev` runs MCPanel inside the dev tool's process tree. Some launchers put
children in a kill-on-close Job Object that forbids breakaway; in that case servers
cannot outlive MCPanel. A normally launched MCPanel (Explorer/Start menu) is not
affected.

## Real Minecraft server test

Running Minecraft requires accepting the [Minecraft EULA](https://aka.ms/MinecraftEULA).
The real-server test runs only when **you** accept it explicitly:

```powershell
$env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA = "yes"
$env:MCPANEL_E2E_SOFTWARE = "paper"      # optional: vanilla (default) | paper | purpur
cargo test -p fake-mc --test real_server -- --nocapture
```

It creates, starts and stops the server, takes a live backup, verifies it, restores it
into the stopped server and boots the restored server. Everything lives in temporary
directories that are deleted afterwards.

`real_players` connects a headless offline-mode client (`tests/common/mc_client.rs`) to
a local test server to check join/leave detection, player commands and file edits:

```powershell
cargo test -p fake-mc --test real_players -- --nocapture
```

`real_content` installs plugins from Modrinth and Hangar onto a real Paper 1.21.11
server and checks that Paper loads them (network access required).

`real_fabric` creates Fabric servers for 26.3 and 1.21.4, installs mods from
Modrinth and checks they load (`--test-threads 1` keeps the downloads sequential).
`real_quilt` boots Quilt 1.21.4 and loads a Fabric mod from Modrinth.
`real_bedrock` sets up Geyser + Floodgate on Paper 1.21.11 (with ViaVersion) and Fabric
26.2, pings the Bedrock listener and checks the UDP port preflight.
`real_performance` checks TPS/MSPT sampling on vanilla 26.3, Paper 1.21.11 and Fabric 26.2.
`real_forge` installs NeoForge 1.21.4 (with a Modrinth mod) and Forge 1.20.1 through
their official installers (several minutes; run with `--test-threads 1`).
`real_crash` kills a real server externally and checks that it is restarted.

The player test client supports only protocol versions whose packet ids were verified (currently
26.3 / protocol 777). Online-mode logins need a Minecraft account and are not automated.

## Live provider tests

```powershell
cargo test -p mcpanel-providers --features live-tests
```

## Installer build behind slow networks

The Tauri bundler downloads NSIS into `%LOCALAPPDATA%\tauri\NSIS` with a short timeout.
If it fails with `timeout: global`, download the files it pins (NSIS 3.11 zip, SHA-1
`EF7FF767E5CBD9EDD22ADD3A32C9B8F4500BB10D`, and `nsis_tauri_utils.dll` v0.5.3, SHA-1
`75197FEE3C6A814FE035788D1C34EAD39349B860`), verify the hashes, and extract them there
(zip contents into `NSIS\`, the DLL into `NSIS\Plugins\x86-unicode\additional\`).
