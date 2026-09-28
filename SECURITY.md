# Security Policy

MCPanel executes local processes, modifies files and will store cloud credentials and
encryption keys. We take security reports seriously.

## Reporting a vulnerability

**Please do not open public issues for security vulnerabilities.**

Use GitHub's **private vulnerability reporting** ("Report a vulnerability" under the
repository's *Security* tab). Include:

- affected version / commit
- a description of the issue and its impact
- reproduction steps or a proof of concept (please avoid destructive payloads)

We aim to acknowledge reports within 7 days and will coordinate disclosure with you.

## Supported versions

MCPanel is pre-1.0. Only the latest release receives security fixes.

## Scope

In scope: the MCPanel application, its IPC/API surface, file handling (path traversal,
symlinks/junctions, archive extraction), secret storage, backup encryption, update
mechanism and the installer.

Out of scope: vulnerabilities in Minecraft itself, third-party plugins/mods, or malware
already running as the same Windows user (such code can access anything the user can).

## Security design summary

- MCPanel runs as a standard user and never requires elevation.
- No network listener is opened in v1 (not even on loopback).
- All server file operations are confined to the server directory through validated
  paths; reparse points are never traversed for writes/deletes.
- Downloads use HTTPS and provider-supplied hashes.
- Secrets live in Windows Credential Manager / DPAPI and never in the database, logs,
  events or audit records.
- No telemetry.

See [`docs/architecture`](docs/architecture/README.md) (threat model chapter) for detail.
