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
