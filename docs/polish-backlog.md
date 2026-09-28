# Polish backlog

Minor visual, animation and wording issues found during feature development. They are
collected here for the dedicated polish/QA phase instead of interrupting feature work.
Functional bugs are fixed immediately and do not belong here.

| Area | Issue | Found |
|---|---|---|
| Players tab | After clicking a tab, the previously focused tab can still look underlined (focus vs. selected styling). | v0.2 players |
| Players actions | A console reply can include an unrelated server line logged at the same moment (e.g. `handleDisconnection() called twice`). Consider filtering WARN lines or matching per command. | v0.2 players |
| Toasts | Several toasts stack tightly in the corner during quick successive actions. | v0.2 backups |
| Backups | Restore dialog content fades in with the dialog; the preview spinner is small. | v0.2 backups |
| Backups | The backups-folder path in the server Backups tab header wraps awkwardly for long paths. | v0.2 backups |
