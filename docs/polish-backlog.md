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
- Bedrock: MCPanel does not add Windows Firewall rules for the UDP port (that needs
  administrator rights); Windows prompts for Java on first use. Decide whether an
  elevated "Allow through firewall" action is wanted.

| Area | Issue | Found |
|---|---|---|
| Players tab | After clicking a tab, the previously focused tab can still look underlined (focus vs. selected styling). | v0.2 players |
| Players actions | A console reply can include an unrelated server line logged at the same moment (e.g. `handleDisconnection() called twice`). Consider filtering WARN lines or matching per command. | v0.2 players |
| Toasts | Several toasts stack tightly in the corner during quick successive actions. | v0.2 backups |
| Backups | Restore dialog content fades in with the dialog; the preview spinner is small. | v0.2 backups |
| Backups | The backups-folder path in the server Backups tab header wraps awkwardly for long paths. | v0.2 backups |
| Plugins | The plugin folder path in the Plugins header wraps; consider a shortened path with a tooltip. | v0.2 plugins |
| Plugins | Search results lack a loading shimmer while typing; results jump when they arrive. | v0.2 plugins |
| Templates | Template cards have no preview of the resolved values for the newest version. | v0.2 templates |
| Fabric | The software line shows the loader as "build #0.19.5"; label it "Loader 0.19.5" for Fabric. | v0.2 fabric |
| Create wizard | The download line shows the weakest hash of a multi-file install (Fabric: SHA-1 of the Mojang jar) without saying the libraries are SHA-512. | v0.2 fabric |
| Notifications | The inbox popover fades in from transparent; a screenshot mid-animation looks washed out. Consider a shorter fade. | v0.3 notifications |
| Settings | The Privacy card lists providers by hand; derive it from the registered providers. | v0.3 notifications |
| Performance | TPS/MSPT history is in memory only (30 minutes) and starts empty after an MCPanel restart. | v0.4 performance |
| Bedrock | Geyser passes the Java MOTD through with quotes (`"A Minecraft Server"`); the connection test shows them verbatim. | v0.3 bedrock |
| Bedrock | The connection card could show this computer's LAN IP next to the Bedrock port (the Overview card already knows it). | v0.3 bedrock |
