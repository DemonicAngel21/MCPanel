# Polish backlog

Minor visual, animation and wording issues found during feature development. They are
collected here for the dedicated polish/QA phase instead of interrupting feature work.
Functional bugs are fixed immediately and do not belong here.

## Open decisions for the QA phase

- The create wizard defaults to the newest Minecraft version even when the software only
  has experimental builds for it (e.g. Paper 26.3: beta). Consider defaulting to the
  newest version with a stable build.
- Provider-recommended JVM flags are applied as published; JDK 26 already deprecates
  `ParallelRefProcEnabled`. MCPanel diagnoses a rejected flag, but could also test flags
  against the chosen Java when a server is created.
- Playit: MCPanel could start/stop the playit agent with `playit start|stop` (the
  installer allows it without elevation). Not built: it publishes the user's tunnels,
  and testing it needs the user's playit account. Needs an explicit decision.
- Encryption: there is no "change passphrase" (create a new Recovery Kit from the key)
  or "remove the key from this computer" action yet. Decide whether both are wanted.
- Bedrock: MCPanel does not add Windows Firewall rules for the UDP port (that needs
  administrator rights); Windows prompts for Java on first use. Decide whether an
  elevated "Allow through firewall" action is wanted.

| Area | Issue | Found |
|---|---|---|
| Players tab | After clicking a tab, the previously focused tab can still look underlined (focus vs. selected styling). | v0.2 players |
| Players actions | A console reply can include an unrelated server line logged at the same moment (e.g. `handleDisconnection() called twice`). Consider filtering WARN lines or matching per command. | v0.2 players |
| Backups | The backups-folder path in the server Backups tab header wraps awkwardly for long paths. | v0.2 backups |
| Plugins | The plugin folder path in the Plugins header wraps; consider a shortened path with a tooltip. | v0.2 plugins |
| Templates | Template cards have no preview of the resolved values for the newest version. | v0.2 templates |
| Fabric | The software line shows the loader as "build #0.19.5"; label it "Loader 0.19.5" for Fabric. | v0.2 fabric |
| Create wizard | The download line shows the weakest hash of a multi-file install (Fabric: SHA-1 of the Mojang jar) without saying the libraries are SHA-512. | v0.2 fabric |
| Settings | The Privacy card lists providers by hand; derive it from the registered providers. | v0.3 notifications |
| Performance | TPS/MSPT history is in memory only (30 minutes) and starts empty after an MCPanel restart. | v0.4 performance |
| Bedrock | Geyser passes the Java MOTD through with quotes (`"A Minecraft Server"`); the connection test shows them verbatim. | v0.3 bedrock |
| Bedrock | The connection card could show this computer's LAN IP next to the Bedrock port (the Overview card already knows it). | v0.3 bedrock |
| Server stop | Stopping a server while it is still *Starting* (e.g. Paper's first-run patching, which ignores `stop`) waits the full graceful timeout (60 s) before terminating it; the UI only shows "Stopping". Consider saying it will be terminated after N s. | v0.4 installer test |
| Installer | The per-user install directory is the data directory (`%LOCALAPPDATA%\MCPanel`). Upgrades and uninstall only touch their own files, so data is safe, but the mix is untidy. | v0.4 installer test |
| Motion | Route/tab fade-ins re-run when the same page is re-selected; harmless but could be skipped. | v0.4 animations |
| Overview | Metric cards keep their chart height (dashed baseline) while the server is stopped; they could collapse. | v0.5 UI pass |
| Dashboard | The Servers stat card has no chart, so it is mostly empty next to CPU and memory. | v0.5 UI pass |
| Activity | On a server's Activity tab, details equal to the server name still show (no name map there). | v0.5 UI pass |
| Server tabs | At narrow widths the active tab is not scrolled into view automatically. | v0.5 UI pass |
| playit | The saved public address is not checked against playit.gg (no public API); a wrong address is only noticed by players. | v0.5 playit |
| Backups | Deleting the last backup of a server leaves its empty per-server folder in the backups folder. | v0.5 installer test |

Resolved in v0.4 animations: toast spacing, restore-preview and plugin-search loading
states (skeletons), inbox popover fade (now a 140 ms scale-in from its anchor).
