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
`real_diagnostics` compiles a test plugin that blocks Paper's server thread and checks the
watchdog crash is recorded with the plugin as suspect (needs a JDK for `javac`).
`real_forge` installs NeoForge 1.21.4 (with a Modrinth mod) and Forge 1.20.1 through
their official installers (several minutes; run with `--test-threads 1`).
`real_crash` kills a real server externally and checks that it is restarted.

The player test client supports only protocol versions whose packet ids were verified (currently
26.3 / protocol 777). Online-mode logins need a Minecraft account and are not automated.

## Cloud storage app registrations

Cloud sign-in needs MCPanel's own OAuth app registrations (public clients, no secret).
Their client IDs are **not** stored in the repository. MCPanel reads, in this order:

1. the runtime environment variable, then
2. the value compiled into the build from the same variable.

| Provider | Variable |
|---|---|
| Google Drive | `MCPANEL_GOOGLE_CLIENT_ID` |
| Google Drive | `MCPANEL_GOOGLE_CLIENT_SECRET` (the Desktop client's secret, see below) |
| Dropbox | `MCPANEL_DROPBOX_CLIENT_ID` |
| Accounts (Firebase) | `MCPANEL_FIREBASE_API_KEY` (the project's Web API key) |

- **Development:** set them in the shell before `pnpm dev` (for example
  `$env:MCPANEL_GOOGLE_CLIENT_SECRET = "…"`), or keep them in a local `.env.local` that
  you load yourself — `.env*` files are git-ignored. A runtime variable overrides the
  compiled-in value.
- **Release:** the release workflow passes the repository secrets
  `MCPANEL_GOOGLE_CLIENT_ID`, `MCPANEL_GOOGLE_CLIENT_SECRET` and
  `MCPANEL_DROPBOX_CLIENT_ID` to `pnpm build`; the values are compiled into the
  binary. Users of an installed release never enter anything.
- **Local release builds** embed whatever is set in your environment at build time. The
  build prints `This build embeds cloud OAuth configuration from the environment: …`
  (names only) so you notice; unset the variables (`Remove-Item Env:MCPANEL_…`) for a
  build that must not contain your app registration.

### MCPanel accounts (Firebase)

Accounts use Firebase Authentication's REST API. Set up a Firebase project:

1. In the Firebase console, create the project **in the same Google Cloud project as
   the Google OAuth Desktop client** (or add that client ID under Authentication →
   Sign-in method → Google → "Safelist client IDs from external projects").
2. Authentication → Sign-in method: enable **Email/Password** and **Google**.
3. Project settings → General → Web API key: that value is `MCPANEL_FIREBASE_API_KEY`.
4. Optional: Authentication → Templates, to customise the verification and password
   reset emails Firebase sends.

The Web API key only identifies the project (Firebase documents that it is not a
secret); it is still kept out of Git like the other values. Without it, the account
step of the first-time setup says accounts are unavailable and can be skipped.

**Why Google has a client secret at all:** Google's token endpoint rejects "Desktop app"
clients without `client_secret` (`invalid_request` "client_secret is missing."), even
with PKCE, although its parameter table lists the field as optional. Google's OAuth
overview says installed apps embed this value in the application, where it "is
obviously not treated as a secret". MCPanel therefore embeds it in releases and treats
it as **non-confidential**: it proves nothing about the caller, PKCE still protects
every sign-in, and a copy extracted from the binary cannot be used to obtain anyone's
tokens without their browser consent. It is still kept out of Git, logs (redacted),
diagnostics, error messages and the UI, and is sent only on Google's code exchange and
refresh. Dropbox uses no secret.

Providers without an ID show "Not configured". Registration details:
docs/architecture/verification-log.md ("Cloud storage OAuth").

## Accessibility check

With `pnpm dev` running for CDP (see above): `node apps/desktop/scripts/cdp.mjs
apps/desktop/scripts/a11y.mjs` runs axe-core on the main pages and prints violations.

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
