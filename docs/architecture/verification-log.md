# External API Verification Log

Every `[VERIFY]` item is checked against the live service / official documentation
before the dependent feature is implemented. Record what was checked, when, and the
result. If the finding contradicts the specification, the spec is updated in the same PR.

## 2026-09-28 — MVP server software sources

### Mojang version manifest — VERIFIED

- `GET https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
  - `latest.release` / `latest.snapshot`; `versions[]` with `id`, `type`
    (`release`, `snapshot`, `old_beta`, `old_alpha`), `url`, `time`, `releaseTime`, `sha1`
    (SHA-1 of the per-version JSON).
  - Observed: `latest.release = "26.3"`, `latest.snapshot = "26.4-snapshot-1"`.
    Year-based versioning (26.x) is in use — confirms that versions must be treated as
    opaque ids ordered by `releaseTime`, never parsed as semver.
- Per-version JSON: `javaVersion.majorVersion` (26.3 → **25**), and
  `downloads.server.{sha1,size,url}` (host `piston-data.mojang.com`).
  Very old versions have no `downloads.server` → treated as "no server download".

### PaperMC downloads — VERIFIED (spec updated)

- The legacy `api.papermc.io/v2` API is **sunset** (returns
  `{"ok":false,"error":"sunset"}`).
- Current API: **Fill v3**, `https://fill.papermc.io/v3`.
  - `GET /v3/projects/paper` → `versions` grouped by family (`"1.21": ["1.21.11", …]`).
  - `GET /v3/projects/paper/versions/{version}` → `version.support.status`,
    `version.java.version.minimum` (e.g. 21), `version.java.flags.recommended` (list),
    `builds` (ids, newest first).
  - `GET /v3/projects/paper/versions/{version}/builds` → list (newest first) of
    `{id, time, channel, commits, downloads}`; `channel` ∈ `STABLE`, `BETA`?, `ALPHA`
    (observed STABLE and ALPHA).
  - `GET …/builds/latest`.
  - `downloads["server:default"] = {name, checksums.sha256, size, url}` (host
    `fill-data.papermc.io`).
  - A descriptive `User-Agent` is sent on every request.

### PurpurMC downloads — VERIFIED

- `GET https://api.purpurmc.org/v2/purpur` → `versions` (ascending).
- `GET /v2/purpur/{version}` → `builds.latest`, `builds.all`.
- `GET /v2/purpur/{version}/{build}` → `result` (`SUCCESS`/`FAILURE`), `timestamp`,
  `md5`.
- `GET /v2/purpur/{version}/{build}/download` → the jar
  (`Content-Disposition: attachment; filename="purpur-<ver>-<build>.jar"`).
- **Only MD5 is published.** MD5 is not collision-resistant; MCPanel verifies it for
  integrity (corruption detection) over HTTPS, and labels Purpur downloads' hash
  strength as "MD5 (integrity only)" rather than "verified".

## 2026-09-28 — Platform and tooling findings during MVP implementation

- **Mojang download hosts:** version JSONs use `piston-data.mojang.com`; very old
  versions (e.g. 1.2.5) still use `launcher.mojang.com`. Both are allow-listed for the
  Vanilla provider. Beta/alpha versions have no server download.
- **Java requirements (Mojang metadata):** 1.12.2 → 8, 1.16.5 → 8, 1.20.1 → 17,
  1.21.4 → 21, 26.3 → 25 (live contract tests).
- **Paper single build endpoint** `…/versions/{v}/builds/{id}` exists; unknown versions
  return 404 `{"ok":false,"error":"version_not_found"}`. Paper 26.x currently only has
  `ALPHA` builds; MCPanel selects the newest experimental build and says so.
- **Job objects:** processes started from some shells/dev tools run inside a
  kill-on-close Job Object that forbids breakaway, so child servers die with MCPanel.
  MCPanel requests `CREATE_BREAKAWAY_FROM_JOB` and falls back when refused. Verified:
  launched normally (outside such a job), a server keeps running after MCPanel is
  killed and is shown as *Detached* on the next launch.
- **Tauri `freezePrototype`:** incompatible with Monaco (it assigns to built-in
  prototypes → "Cannot assign to read only property 'toString'"). Disabled; the strict
  CSP, no remote content and no Node/FS/shell plugins in the webview remain.
- **Monaco 0.57 exports map:** worker entry points are `monaco-editor/editor/editor.worker`
  and `monaco-editor/language/json/json.worker` (no `esm/vs/` prefix).
- **SQLx 0.9** rejects non-`'static` SQL strings at compile time (injection guard); all
  queries are static.
- **Tauri CLI 2.12 NSIS toolchain:** pinned to NSIS 3.11 zip (SHA-1 EF7FF767…) and
  `nsis_tauri_utils.dll` v0.5.3 (SHA-1 75197FEE…); see `docs/development.md` for the
  slow-network workaround.
- **Named job objects** do not survive the last handle closing, so an orphaned server is
  terminated by walking the process tree (PID + start-time verified), not by re-opening
  its job.

## 2026-09-28 — Real server end-to-end run

- `real_server` test (Minecraft EULA accepted by the project owner for the test
  environment): **Vanilla 26.3** on JDK 25.0.4 downloaded (SHA-1 verified), created,
  started (ready in ~8 s), accepted a `list` command, stopped gracefully with exit code
  0, and generated `server.properties`. No process, port or file left behind.

## 2026-09-28 — Live backup against Vanilla 26.3

- `save-off` / `save-all flush` / `save-on` work on 26.3, but **26.x logs command
  feedback with a `System chat: ` prefix** (`System chat: Saved the game`,
  `System chat: Automatic saving is now disabled`). The `save_complete` dialect pattern
  now accepts the optional prefix (player chat is `<name> …` and cannot match).
  The first attempt timed out waiting for the unprefixed line — found by the real test.
- Result: live backup of a fresh 26.3 world = 74 files / 125 MB, nothing skipped
  (`world/session.lock` is locked by the JVM and excluded by design); archive verified;
  restored into the stopped server, which booted in ~7 s and stopped with exit code 0.
- Join/leave messages: see the players entry below.

## 2026-09-28 — Players against Vanilla 26.3 (headless test client)

- Protocol for the test client verified at minecraft.wiki (Java Edition protocol,
  Packets): 26.3 = **protocol 777**; handshake/login/configuration/play packet ids as
  used in `tools/fake-mc/tests/common/mc_client.rs`. The client reads the protocol from
  a Server List Ping and refuses unknown protocols instead of guessing.
- 26.3 logs `System chat: <name> joined the game` / `… left the game` and all command
  feedback with the `System chat: ` prefix (`Made X a server operator`, `Nothing
  changed. The player is already an operator`, `Added X to the whitelist`,
  `Banned X: reason`, `Kicked X: reason`, `Whitelist is now turned off`). Join/leave
  patterns now accept the prefix — before this fix online players were not tracked on
  26.x.
- Offline-mode logins print no `UUID of player …` line; `usercache.json` holds the UUID,
  which equals the offline UUID (`UUID.nameUUIDFromBytes("OfflinePlayer:"+name)`).
- **26.3 generates `white-list=true` for new servers.** The property schema reflects this
  from 26.3; earlier 26.x releases were not checked.
- Files edited by MCPanel while the server is stopped (ops, whitelist, white-list
  property) were loaded by the real server on the next start.
- Mojang profile lookup verified live: `api.minecraftservices.com/minecraft/profile/
  lookup/name/{name}` and `api.mojang.com/users/profiles/minecraft/{name}` → `{id, name}`,
  404 for unknown names.
- Not automatable: online-mode (authenticated) logins require a real Minecraft account.

## 2026-09-28 — Modrinth and Hangar (plugin manager)

- **Modrinth API v2** (live): `/v2/search` (facets `project_type`, `categories`
  = loaders, `versions`; indexes relevance/downloads/updated; multi-platform projects
  report `project_type: "mod"` but match `project_type:plugin`), `/v2/project/{id}`,
  `/v2/project/{id}/version?loaders&game_versions` (files with `hashes.sha512`/`sha1`,
  `cdn.modrinth.com` URLs, dependencies `{version_id?, project_id?, file_name?,
  dependency_type}`), `/v2/version/{id}`, `POST /v2/version_files` and
  `POST /v2/version_files/update` (by SHA-512). Rate limit 300/min.
- **Hangar API v1** (live): `/api/v1/projects?q&platform=PAPER&version&sort`,
  `/api/v1/projects/{slugOrId}` (404 when unknown), `…/versions?platform&
  platformVersion&channel` (`downloads.PAPER.fileInfo.sha256Hash`, `downloadUrl` on
  `hangarcdn.papermc.io` or only `externalUrl`), `…/versions/{name}` (names may
  contain `+`). No lookup by hash.
- Real Paper 1.21.11: Chunky (Modrinth) installed while stopped and loaded; ViaVersion
  (Hangar) installed while running, queued, applied on stop and loaded on the next
  start; a plugin disabled while running was not loaded after the restart.

## 2026-09-28 — Crash recovery against Vanilla 26.3

- The server's Java process was killed from outside MCPanel (`taskkill /T /F`): MCPanel
  classified it as a crash (exit code 1, "exited unexpectedly"), and the restart policy
  (2 s delay) brought 26.3 back up in ~9 s. Watchdog detection uses vanilla's
  "Considering it to be crashed, server will forcibly shutdown" line.

## 2026-09-28 — Templates and JVM flags

- PaperMC "Aikar's flags" (docs.papermc.io/paper/aikars-flags) are accepted by JDK
  25.0.4; JDK 26.0.1 warns that `ParallelRefProcEnabled` is deprecated and will likely be
  removed. Templates therefore carry no JVM flags (providers publish recommended flags),
  and a rejected flag is diagnosed: JDK 25 prints `Unrecognized VM option '<name>'` or
  `Unrecognized option: <opt>`, then `Could not create the Java Virtual Machine`.
- Every built-in template resolves without skipped entries for 1.12.2, 1.16.5, 1.20.1,
  1.21.4 and 26.3 (unit test), using the version-split property values of the schema.
- Created a Paper 26.3 server from the Creative template in the app: properties
  (gamemode, difficulty, level-type, spawn-monsters, allow-flight, pvp) were written and
  the backup schedule and restart policy were applied.

## 2026-09-28 — Fabric

- **Fabric meta API v2** (live): `/v2/versions/game` (`{version, stable}`),
  `/v2/versions/loader/{game}` (`[{loader{version,stable}, …}]`),
  `/v2/versions/loader/{game}/{loader}/server/json` (profile: `mainClass`
  `net.fabricmc.loader.impl.launch.knot.KnotServer`, `libraries[{name, url, sha512?,
  size?}]`; the loader — and intermediary for obfuscated versions — have no hash in the
  profile, but `maven.fabricmc.net` publishes `<jar>.sha512`). 26.x profiles have no
  intermediary.
- The official installer verifies the vanilla jar's SHA-1 but downloads libraries
  without any hash check (fabric-installer `ServerInstaller`), and the meta
  "server launcher" jar has no published hash — so MCPanel installs Fabric natively with
  every file verified.
- Launch: Knot builds its class path from `java.class.path` only, so a `-jar` launch
  jar with a manifest `Class-Path` fails ("couldn't locate the game", then "trying to
  load FabricLoaderImpl from target class loader"). MCPanel passes the libraries with
  `-cp`, the game jar with `-Dfabric.gameJarPath=server.jar` (what
  `FabricServerLauncher` sets) and starts the profile's main class. Verified on real
  servers: Fabric 26.3 (unobfuscated) and 1.21.4 (intermediary, bundler).
- Modrinth mod search uses the `server_side:required|optional` facet (Sodium is
  excluded); a jar whose `fabric.mod.json` says `"environment": "client"` is refused
  (verified with Sodium for 26.3). Fabric API and Lithium installed and loaded.

## 2026-09-29 — Forge and NeoForge

- NeoForge Maven (`maven.neoforged.net/releases/net/neoforged/neoforge`): metadata
  versions `21.4.157` (= 1.21.4), `26.3.0.31-beta` (= 26.3); installers with
  `.sha1/.sha256/.sha512`. Installer CLI (`--help`): `--install-server [dir]`, hash
  checks on unless `--skip-hash-check`. Output: `libraries/net/neoforged/neoforge/<v>/
  win_args.txt` (`-p … --add-modules … <main>`), `run.bat` = `java @user_jvm_args.txt
  @…/win_args.txt`.
- Forge: `promotions_slim.json` (`<mc>-recommended|latest`), Maven metadata
  `<mc>-<forge>`, installers with `.sha512`; `--installServer`. 1.16.5 and 1.12.2
  installers (run on JDK 25) write `forge-<mc>-<forge>.jar` + the vanilla jar and log
  "Checksum Validated" for downloads.
- Real servers through MCPanel: NeoForge 1.21.4 (install ~155–180 s, boot ~12 s,
  FerriteCore from Modrinth loaded) and Forge 1.20.1 (install ~127 s, boot ~31 s), both
  on JDK 25. Old Forge (1.12.2/1.16.5) needs Java 8 to *run*; not run here (no Java 8
  installed).

## 2026-09-29 — Quilt

- Meta API v3 (`https://meta.quiltmc.org/v3`): `/versions/game` → `{version, stable}`;
  `/versions/loader/{game}` → `[{loader:{version,…},…}]`, **not sorted** and without a
  stable flag; the global `/versions/loader` list is newest first.
  `/versions/loader/{game}/{loader}/server/json` has the Fabric profile format, main
  class `org.quiltmc.loader.impl.launch.knot.KnotServer`, libraries from
  `https://maven.fabricmc.net/` and `https://maven.quiltmc.org/repository/release/`
  without hashes; both repositories publish `.sha512` files.
- Real server through MCPanel: Quilt Loader 0.30.1 on 1.21.4 booted in ~13 s on JDK 25
  (`-cp` launch with `-Dloader.gameJarPath` / `-Dfabric.gameJarPath`), and Lithium
  (a Fabric mod) from Modrinth loaded.

## 2026-09-29 — GeyserMC / Bedrock

- Download API `https://download.geysermc.org/v2` (documented at
  geysermc.org/wiki/api/downloads): `/projects` (geyser, floodgate, …),
  `/projects/{p}` → `versions[]` oldest first, `/projects/{p}/versions/{v}/builds` →
  `builds[]` oldest first with `build`, `time`, `channel` ("default"),
  `downloads{platform:{name, sha256}}`; `/…/builds/{b}` (404 if unknown) and
  `/…/builds/{b}/downloads/{platform}` (the jar; SHA-256 matched). `…/latest/builds/latest`
  answers with a 302 to the concrete build. Geyser platforms: spigot, fabric, neoforge,
  velocity, bungeecord, standalone, viaproxy; Floodgate: spigot, bungee, velocity.
- Geyser docs: Geyser 2.11.x supports Bedrock 26.30–26.51 and Java 26.2; Geyser-Spigot
  runs on Spigot/Paper 1.20.5+; Geyser-Fabric/-NeoForge only on the latest Java version
  (Modrinth tags the mod builds with it, e.g. 26.2). Floodgate for Fabric/NeoForge comes
  from Modrinth (GeyserMC organisation).
- Observed on Paper 1.21.11 with Geyser 2.11.3-b1247 + Floodgate 2.2.5-b141: config
  `plugins/Geyser-Spigot/config.yml` (config-version 8; `bedrock.port`,
  `java.auth-type` — "online" by default even with Floodgate installed), Floodgate key
  `plugins/floodgate/key.pem`; log line `Started Geyser on UDP port <n>`; without
  ViaVersion: "Your server software does not support the Java version that Geyser
  requires (26.2, 26.1.1, 26.1.2). Please install ViaVersion …" (gone with ViaVersion
  5.12.0). A config containing only `bedrock.port` and `java.auth-type` (no
  `config-version`) is completed by Geyser with defaults and comments, keeping those
  values; the listener binds the configured port.
- RakNet unconnected ping: `0x01, i64 time, MAGIC 00ffff00fefefefefdfdfdfd12345678, i64
  guid` → `0x1c, i64 time, i64 guid, MAGIC, u16 len, "MCPE;motd;2193;26.51;0;20;guid;
  sub-motd;Survival;1;19132;19132;"`.
- Through MCPanel: Paper 1.21.11 (Geyser + Floodgate + ViaVersion) and Fabric 26.2
  (Geyser-Fabric 2.11.3 + Floodgate-Fabric 2.2.6 + Fabric API) both answered the ping on
  the configured port with Floodgate authentication.

## 2026-09-29 — Playit.gg spike

- Source: github.com/playit-cloud/playit-agent @ 4c27794 (v1.0.10, BSD-2-Clause);
  winget `DevelopedMethods.playit` 1.0.10 (Developed Methods LLC). Windows install:
  `C:\Program Files\playit_gg\bin\playit.exe` + service `playitd` (pipe
  `\\.\pipe\playitd-system`; the installer grants authenticated users start/stop).
- CLI (`playit --help`, run locally): version, attach, start, stop, status, reset,
  secret-path, setup, account login-url, claim generate|url|exchange. `playit status`
  prints "The playit service is not running." or "playit service status:" followed by
  `Phase: <running|starting|waiting for secret|invalid secret|disabled over limit|
  stopping|error>`, `Secret configured: <bool>` and other lines (source:
  `packages/playit-cli/src/client.rs`). Verified locally: version `1.0.10`, status
  "not running". The agent was not started or claimed.
- Tunnel creation exists only in the agent's internal API client (`api_client`, e.g.
  `v1_tunnels_create`); playit.gg documents no third-party API. Decision: ADR-0007.

## 2026-09-30 — Playit.gg re-check

- playit 1.0.10 local CLI: no tunnel subcommands; `claim url <CODE> [--name] [--type]`,
  `claim exchange <CODE> [--wait]` (prints the secret key — not used by MCPanel),
  `setup` (prints "Open this link to finish setting up playit:" and
  `https://playit.gg/claim/<10 hex>`, then waits; verified against a separate
  `playitd.exe --secret-path <tmp> --socket-path \\.\pipe\<test>`), `account login-url`
  (guest accounts only: "Fail(AccountIsNotGuest)" for a verified account).
- `playit start` / `stop` on the installed service as a normal user: "The playit service
  started." / "The playit service stopped."; status phases observed: starting → running.
- `playit status` for a non-default socket starts with "playitd daemon status for socket".
- Private API and IPC: see ADR-0007 amendment (source `packages/api_client/src/api.rs`,
  `playit-ipc`; playit-minecraft-plugin `PlayitManager.java`; terms updated 2026-02-20).

## 2026-09-29 — TPS / MSPT commands

- `/tick` was added in Java Edition 1.20.3 (23w43a) (minecraft.wiki, Commands/tick).
- Real output (MCPanel console, commands sent on stdin):
  - Vanilla 26.3: "System chat: The game is running normally", "System chat: Target tick
    rate: 20.0 per second.", unprefixed "Average time per tick: 0.3ms (Target: 50.0ms)",
    "System chat: Percentiles: P50: 0.3ms P95: 0.4ms P99: 1.4ms. Sample: 100".
  - Vanilla 1.21.4: same without "System chat: " and with ", sample: 100".
  - Paper 1.21.11: `tick query` as vanilla 1.21.4; `tps` → "TPS from last 1m, 5m, 15m:
    20.0, 20.0, 20.0"; `mspt` → "Server tick times (avg/min/max) from last 5s, 10s, 1m:"
    and "◴ 0.4/0.3/4.8, 1.0/0.3/35.9, 1.0/0.3/35.9".
  - Fabric 26.2: `tick query` as vanilla 26.3.
- Sampling through MCPanel verified on those three servers (`real_performance`).

## 2026-09-29 — Spiget

- API `https://api.spiget.org/v2` (swagger: github.com/SpiGetOrg/Documentation, Apache
  2.0, operated by inventivetalent; not SpigotMC). Search
  `/search/resources/{q}?field=name&size&page&sort&fields`, listing `/resources`, details
  `/resources/{id}` (404 unknown), `/resources/{id}/versions/latest`. Searches without
  results answer 404. Resources: `external`, `premium` (omitted when false),
  `file{type ".jar"|".sk"|"external", url, externalUrl?}`, `testedVersions`,
  `updateDate` (unix seconds). No file hashes anywhere.
- Downloads: `/resources/{id}/download` → 302 `https://cdn.spiget.org/file/spiget-resources/{id}.jar`
  (latest stored file); `/resources/{id}/versions/{v}/download` → 302 to
  spigotmc.org (Cloudflare, not automatable); `…/download/proxy` documented as strictly
  rate limited.
- Through MCPanel: LuckPerms (28140) from Spiget installed on Paper 1.21.11 (plan warned
  "unverified"), descriptor LuckPerms 5.5.71, Paper enabled it.

## 2026-09-29 — Backup encryption

- `age` crate 0.12.1 (MIT OR Apache-2.0, str4d/rage): `x25519::Identity::generate`,
  `Encryptor::with_recipients` / `with_user_passphrase` (scrypt), `Decryptor`,
  `armor::ArmoredWriter`/`ArmoredReader`. Round trips, wrong-key and tamper detection
  covered by unit tests; encrypted backup → verify → restore → key loss → Recovery Kit
  import covered end to end (`backups.rs`).
- Windows Credential Manager (`CREDENTIALW` docs): generic credentials,
  `CredentialBlobSize` ≤ `CRED_MAX_CREDENTIAL_BLOB_SIZE` (5×512 = 2560 bytes);
  `CRED_PERSIST_LOCAL_MACHINE` = this user, this computer, not roaming. An age identity
  is 74 bytes. Round trip tested against the real Credential Manager with a temporary
  test entry that the test deletes.

## 2026-09-29 — Paper watchdog and crash analysis

- Paper 1.21.11 with a test plugin blocking the server thread (spigot.yml
  `timeout-time: 15`): after 10 s "--- DO NOT REPORT THIS TO PAPER - THIS IS NOT A BUG OR
  A CRASH ---" dumps; at the timeout "The server has stopped responding! This is
  (probably) not a Paper bug.", a "Server thread dump" with frames like
  `CrashTest.jar//com.example.mcpaneltest.CrashTest.blockServerThread(CrashTest.java:14)`
  (class-loader name = plugin jar), then "Stopping server", world saving and exit code
  70. No crash-reports file is written.

## 2026-09-30 — Cloud storage OAuth (Google Drive, OneDrive, Dropbox)

All three use the authorization code flow with PKCE (`S256`) through the system browser
and a loopback redirect; MCPanel sends **no client secret**. Refresh tokens are stored in
the Windows Credential Manager (`MCPanel/cloud-<provider>-refresh-token`), access tokens
only in memory. Without an app registration, each real token endpoint was called with an
invalid client ID (live test `cloud_token_endpoints_reject_an_unknown_client_clearly`):
Google answered "The OAuth client was not found", Microsoft `AADSTS9002313`, Dropbox
`invalid_client: Invalid client_id` — endpoints and error mapping confirmed. A real
sign-in has **not** been tested (needs the app registrations).

**Google Drive** (developers.google.com/identity/protocols/oauth2/native-app;
workspace/drive/api/guides/api-specific-auth; support.google.com/cloud/answer/15549945)
- OAuth client type **Desktop app**. Redirect `http://127.0.0.1:{port}` on any free port;
  loopback redirects are not pre-registered ("`localhost` … may cause issues with client
  firewalls"). MCPanel uses `http://127.0.0.1:{port}/`.
- Endpoints: `https://accounts.google.com/o/oauth2/v2/auth`,
  `https://oauth2.googleapis.com/token`, `https://oauth2.googleapis.com/revoke`.
- `client_secret` is documented as **Optional** for code exchange and refresh
  ("installed apps … cannot keep secrets"); refresh tokens are always returned for
  installed apps.
- **Actual token endpoint behaviour differs from the parameter table** (checked
  2026-09-30 with a public installed-app client ID and a deliberately invalid code): without
  `client_secret` both the code exchange and the refresh answer HTTP 400
  `{"error":"invalid_request","error_description":"client_secret is missing."}` — the
  secret is checked before the code. With a wrong secret: `invalid_client` "The provided
  client secret is invalid." Google's OAuth overview says of installed apps: "a client
  secret, which you embed in the source code of your application. (In this context, the
  client secret is obviously not treated as a secret.)" MCPanel currently sends none; a
  Google sign-in with a "Desktop app" client therefore failed at the code exchange with
  that message (shown as Google's answer).
- Decision (2026-09-30): releases embed the Desktop client secret
  (`MCPANEL_GOOGLE_CLIENT_SECRET` at build time) and send it on the code exchange and
  refresh, next to PKCE. Checked against the real endpoint with a placeholder secret:
  both requests then answer `invalid_client` "The provided client secret is invalid."
  (the secret is transmitted and validated; the placeholder never appears in MCPanel's
  messages).
- Scope `https://www.googleapis.com/auth/drive.file` — **non-sensitive** (files the app
  creates or the user opens with it); `about.get` accepts it (`fields` required).
- Publishing status "Testing": at most 100 test users and authorizations **expire after 7
  days**; "In production" needs no verification for non-sensitive scopes (the
  unverified-app screen and 100-user cap apply only to sensitive/restricted scopes).

**Microsoft OneDrive** — *postponed on 2026-09-30: removed from the product; kept here
for when it is added back* (learn.microsoft.com: entra/identity-platform/reply-url,
v2-oauth2-auth-code-flow; onedrive/developer/rest-api/concepts/special-folders-appfolder)
- Register under **Mobile and desktop applications** (public client). `http://localhost`
  is allowed and "the port component … is ignored for the purposes of matching a
  localhost redirect URI"; the path must match (case-sensitive); query parameters are not
  allowed for apps that sign in personal accounts. (http + `127.0.0.1` needs the manifest
  editor; MCPanel uses `localhost`.) MCPanel uses `http://localhost:{port}/mcpanel/oauth`
  → register `http://localhost/mcpanel/oauth`. The listener binds 127.0.0.1 and ::1.
- Endpoints: `https://login.microsoftonline.com/common/oauth2/v2.0/authorize|token`
  (`common` = personal + work/school accounts); `response_mode=query`.
- "Public clients … must not use secrets or certificates when redeeming an authorization
  code." A refresh token is returned only with `offline_access`; refresh tokens can be
  rotated (MCPanel stores the new one).
- Scopes (delegated): `offline_access`, `User.Read` (`GET /me`),
  `Files.ReadWrite.AppFolder` (`/drive/special/approot`, created in the user's `Apps`
  folder and named after the app registration).
- No token revocation endpoint for this flow: disconnect deletes the stored token and
  links to https://account.live.com/consent/Manage.

**Dropbox** (docs.dropboxapi.com/dropbox-api/docs/oauth; dropbox.tech PKCE and offline
access posts; Dropbox staff answers on dropboxforum.com)
- PKCE: `code_challenge`, `code_challenge_method=S256`; the exchange passes
  `code_verifier` instead of `client_secret`; `token_access_type=offline` returns a
  refresh token; refresh with `grant_type=refresh_token` + `client_id`.
- Redirect URIs must be **pre-registered and match exactly, including the port**
  (no variable loopback port). MCPanel uses the first free of
  `http://localhost:43917/mcpanel/oauth`, `…:43918/…`, `…:43919/…` — all three must be
  registered.
- Endpoints: `https://www.dropbox.com/oauth2/authorize`,
  `https://api.dropboxapi.com/oauth2/token`, `POST /2/users/get_current_account`,
  `POST /2/auth/token/revoke`.
- Scoped app, access type **App folder**; scopes `account_info.read` (required for
  user-linked apps), `files.metadata.read`, `files.content.read`, `files.content.write`.
- Development status: up to 500 linked users; after 50, two weeks to obtain production
  approval.

## Pending (verify before the dependent phase)

| Item | Phase |
|---|---|
| CurseForge API key terms and distribution flags | v0.3+ |
| Cloud sign-in with real app registrations; upload APIs (resumable upload per provider) | v0.4 |
| Adoptium API (Java downloader) | later |
| Tauri NSIS per-user install directory default | v0.5 |
