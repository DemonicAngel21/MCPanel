# MCPanel

**A modern Windows desktop control panel for local Minecraft Java servers.**

MCPanel makes running Minecraft servers on your own PC easy — create a server in a
minute, start it, watch the live console, edit files and `server.properties` safely —
while keeping the depth power users expect: per-server Java runtimes, JVM tuning,
multiple servers, and (on the roadmap) backups, plugin/mod management, Playit.gg tunnels
and one-click Bedrock support via Geyser/Floodgate.

> **Status:** early development (pre-`v0.1.0`). Not ready for production use.

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

See [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow, commit conventions and
branching model.

## Security

Please report vulnerabilities privately — see [SECURITY.md](SECURITY.md).

## Minecraft EULA

MCPanel never accepts the [Minecraft EULA](https://aka.ms/MinecraftEULA) on your behalf.
You are asked to read and accept it explicitly when creating a server.

MCPanel is not affiliated with Mojang Studios or Microsoft.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
