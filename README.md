# MCPanel

**A modern Windows desktop control panel for local Minecraft Java servers.**

MCPanel makes running Minecraft servers on your own PC easy — create a server in a
minute, start it, watch the live console, edit files and `server.properties` safely —
while keeping the depth power users expect: Vanilla, Paper, Purpur, Fabric, Quilt,
NeoForge and Forge; per-server Java runtimes and JVM tuning; plugins and mods from
Modrinth, Hangar, GeyserMC and SpigotMC; live, scheduled and encrypted backups; crash
auto-restart with crash analysis; TPS/MSPT; one-click Bedrock crossplay with
Geyser/Floodgate; notifications; and playit.gg guidance for playing over the internet.

> **Status:** feature-complete for the planned roadmap, in final polish/QA. Not yet
> released. User guide: [`docs/user`](docs/user/README.md).

## Principles

- **Safe by default** — no network exposure, no silent data loss, validated file access.
- **Honest** — MCPanel never shows a metric or capability it cannot actually provide.
- **No lock-in** — servers stay ordinary folders you can run without MCPanel.
- **Local first** — no MCPanel accounts, no cloud service, **no telemetry**.

## Architecture at a glance

```
React UI ──typed IPC──▶ Application API ──▶ MCPanel Core (headless Rust)
                                              │
                    ┌─────────────┬───────────┼────────────┬──────────────┐
                    ▼             ▼           ▼            ▼              ▼
               SQLite (db)   Providers   Platform     Secret store    Event bus
                           (Mojang, Paper, (Windows)
                            Purpur, ...)
```

The UI contains no server-management logic. The core does not depend on the UI or on
Tauri. External services live behind provider interfaces. See
[`docs/architecture`](docs/architecture/README.md) for the full specification and
[`docs/adr`](docs/adr) for architecture decision records.

## Repository layout

| Path | Contents |
|---|---|
| `apps/desktop` | Tauri 2 host (`src-tauri`) and React/TypeScript UI (`src`) |
| `crates/mcpanel-core` | Domain model, services, ports (traits), events, jobs — headless |
| `crates/mcpanel-api` | Application API: DTOs, error codes, command facade |
| `crates/mcpanel-db` | SQLite persistence and migrations |
| `crates/mcpanel-providers` | Provider implementations (server software, downloads, ...) |
| `crates/mcpanel-platform` | Platform adapters (Windows first) |
| `data/` | Versioned data files (property schemas, Java compatibility, log dialects) |
| `tools/fake-mc` | Test double that behaves like a Minecraft server |
| `docs/` | Architecture, ADRs, user documentation |

## Development

Requirements (Windows 10/11 x64):

- Rust stable (MSVC toolchain), pinned via `rust-toolchain.toml`
- Node.js ≥ 24 and pnpm (version pinned in `package.json` `packageManager`)
- Visual Studio Build Tools with the C++ workload, Windows SDK
- Microsoft Edge WebView2 Runtime (preinstalled on Windows 11)
- A Java runtime to actually run Minecraft servers

```powershell
pnpm install
pnpm dev          # run the desktop app in development mode
pnpm check        # lint + typecheck + frontend tests
cargo test --workspace
pnpm build        # production build + NSIS installer
```

See [docs/development.md](docs/development.md) for isolated data directories, UI
end-to-end runs with the `fake-mc` test double and the opt-in real-server test, and
[CONTRIBUTING.md](CONTRIBUTING.md) for the workflow, commit conventions and branching
model.

## What works today (v0.1 development)

- Create Vanilla, Paper and Purpur servers (verified downloads, explicit EULA consent)
  or import existing server folders
- Start, stop (graceful, then forced after a timeout), restart; crash and error
  diagnosis; servers keep running when MCPanel closes and are re-detected
- Live console that stays responsive under heavy output, with filters, history and
  autocomplete
- File explorer and Monaco editor with safe paths, trash, zip/unzip, uploads
- Version-aware `server.properties` editor, launch settings (Java, memory, JVM flags)
- Java runtime detection, system and process metrics, activity log, Ctrl+K palette

Planned for later versions: backups, plugins/mods, players, Playit.gg, Bedrock
(Geyser/Floodgate), cloud backups, notifications — see the roadmap in the
[specification](docs/architecture/README.md#25-roadmap).

## Security

Please report vulnerabilities privately — see [SECURITY.md](SECURITY.md).

## Minecraft EULA

MCPanel never accepts the [Minecraft EULA](https://aka.ms/MinecraftEULA) on your behalf.
You are asked to read and accept it explicitly when creating a server.

MCPanel is not affiliated with Mojang Studios or Microsoft.

## License

MCPanel is created by DemonicAngel21 and licensed under the [MIT License](LICENSE).
Copyright © 2026 DemonicAngel21.

Third-party dependencies keep their own licenses (for example MIT, Apache-2.0 or
BSD); see each dependency.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in MCPanel by you shall be licensed under the MIT License, without any
additional terms or conditions.
