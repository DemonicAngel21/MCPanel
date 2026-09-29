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

## Pending (verify before the dependent phase)

| Item | Phase |
|---|---|
| Vanilla `/tick query` output format; Paper `tps`/`mspt` output | v0.4 |
| CurseForge API key terms and distribution flags | v0.3+ |
| Playit agent interfaces / supported tunnel management | v0.3 spike |
| GeyserMC download API, config schema, Forge support status | v0.3 |
| OneDrive (Graph), Dropbox, Google Drive APIs + app verification | v0.4 |
| Adoptium API (Java downloader) | later |
| Tauri NSIS per-user install directory default | v0.5 |
| Credential Manager blob size limit | v0.4 |
