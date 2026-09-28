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

## Pending (verify before the dependent phase)

| Item | Phase |
|---|---|
| Vanilla `/tick query` output format; Paper `tps`/`mspt` output | v0.4 |
| Modrinth API v2, Hangar API | v0.2 |
| Fabric meta server launcher endpoint | v0.2 |
| Forge / NeoForge installer & argfile layout per version | v0.3 |
| CurseForge API key terms and distribution flags | v0.3+ |
| Playit agent interfaces / supported tunnel management | v0.3 spike |
| GeyserMC download API, config schema, Forge support status | v0.3 |
| OneDrive (Graph), Dropbox, Google Drive APIs + app verification | v0.4 |
| Adoptium API (Java downloader) | later |
| Tauri NSIS per-user install directory default | v0.5 |
| Credential Manager blob size limit | v0.4 |
