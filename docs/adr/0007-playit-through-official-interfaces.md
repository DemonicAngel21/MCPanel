# ADR-0007: Playit.gg only through its official program and website

- Status: Accepted
- Date: 2026-09-29

## Context

Users want friends outside their network to join. Playit.gg provides tunnels through
its agent (`playit` CLI + `playitd` service, BSD-2-Clause, installed by the official
installer or winget). The agent's source contains an HTTP client for playit's API,
including tunnel creation, but that API is not documented or offered to third-party
applications. MCPanel's rules forbid undocumented/private APIs and claims of automation
that does not exist.

## Decision

MCPanel detects the installed agent (`%ProgramFiles%\playit_gg\bin\playit.exe` or
PATH), reports its version and state through the documented CLI (`playit version`,
`playit status`), and links to the official download page and dashboard, where the
user creates tunnels. `TunnelCaps::can_create_via_api` is `false`. MCPanel does not
start, stop, claim or reset the agent: those change what is published from the user's
account and are left to the user (or a later, explicitly authorised and tested
feature).

## Amendment (2026-09-30)

Re-investigated against playit 1.0.10 and the playit-agent source (4c27794):

- Still no public API: staff, 2026-05-04: "We have an API, just not public. Will release
  when we decide to, no ETA" (playit-agent issue #150). The 1.0 CLI dropped the
  `tunnels prepare/list` commands of 0.15 (issue #155 closed without restoring them).
- Tunnel CRUD exists in playit's private HTTP API (`/v1/tunnels/create`, `/tunnels/list`,
  `/tunnels/update`, `/tunnels/delete`, authenticated with the agent's secret key), which
  playit's own Minecraft plugin uses. Tunnel addresses are also available from playitd's
  internal named-pipe IPC (`get_state`). Neither is documented or offered to third parties;
  the playit terms (§3.1) restrict API use, and using the private API would mean MCPanel
  reading the agent secret from `%PROGRAMDATA%\playit_gg\playit.toml`.
- The official CLI supports `start`/`stop` (the installer allows users to control the
  service) and `setup` (claim flow; the secret goes directly to the service).

Decision update: on the user's explicit request MCPanel now starts and stops the agent
(`playit start|stop`) and links an unlinked agent (`playit setup`, discarding its output
except the `https://playit.gg/claim/<hex>` URL). The user can save a tunnel's public
address per server. Tunnel creation, listing and changes stay in the playit.gg dashboard
(`TunnelCaps::can_manage_tunnels = false`) until playit publishes a supported API.

## Consequences

Setting up internet access needs a few manual steps in the playit.gg dashboard. If
playit.gg publishes a supported API, a `TunnelProvider` can add creation behind the
same capability flag. The `playit status` text format is parsed defensively; unknown
output is reported as "state unknown" rather than guessed.
