# ADR-0005: Minecraft files are the source of truth for Minecraft configuration

- Status: Accepted
- Date: 2026-09-28

## Context

Users (and plugins) edit `server.properties`, `ops.json`, `whitelist.json` and bans
outside MCPanel. Duplicating that data into the database creates drift.

## Decision

MCPanel reads and writes those files (losslessly, preserving comments/order/unknown
keys). The database stores only MCPanel's own data (metadata, history, jobs, audit,
secret references) and optional caches that can be rebuilt.

## Consequences

Servers remain ordinary folders usable without MCPanel. Parsers must be robust to
arbitrary (untrusted) file content.
