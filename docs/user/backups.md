# Backups

Backups are ZIP files you can open in Explorer (or `.zip.age` when encrypted). They are
stored in `%USERPROFILE%\MCPanel\Backups` by default, one folder per server; the global
**Backups** page can move this location (it may not be inside a server folder).

## Live backups

A backup of a running server pauses saving (`save-off`), flushes the world (`save-all
flush`), archives the files and turns saving back on — even if something fails. While a
backup runs the server cannot be started or restarted. Files locked by another program are
skipped and listed.

## Schedules and retention

Each server's **Backups** tab has a schedule (every 15 minutes to every week) and keeps the
newest backups plus the newest of each day, week and month (grandfather–father–son). An
optional **size limit** removes the oldest scheduled backups when they use more space; the
newest is always kept. Manual and pre-restore backups are never deleted automatically.
*Skip when the server has not run* avoids identical backups.

## Verify and restore

**Verify** reads the whole archive and checks every file against the manifest. **Restore**
stops nothing on its own: stop the server first. MCPanel then takes a *pre-restore* backup,
extracts into a staging folder, checks every file's hash and only then swaps the folder.
If the backup was taken with different server software, the server's software is switched
back to match.

## Encryption

Settings → **Backup encryption** creates a key that stays in the Windows Credential Manager
of your account on this PC, and saves a **Recovery Kit**: the key protected by your
passphrase (at least 12 characters). New backups are then encrypted (`.zip.age`, the
open *age* format).

- Keep the Recovery Kit and the passphrase safe and separate. Without them encrypted
  backups cannot be opened — not even by MCPanel.
- On another PC (or after reinstalling Windows), use **Import Recovery Kit**.
- Verify and restore decrypt to a temporary file next to the backup, which is always
  removed afterwards.

## Sensitive files

Backups include highly sensitive files such as the Floodgate key (so a restore is
complete) and are marked **Sensitive**. Do not share them.
