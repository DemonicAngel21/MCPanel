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

## Consequences

Setting up internet access needs a few manual steps in the playit.gg dashboard. If
playit.gg publishes a supported API, a `TunnelProvider` can add creation behind the
same capability flag. The `playit status` text format is parsed defensively; unknown
output is reported as "state unknown" rather than guessed.
