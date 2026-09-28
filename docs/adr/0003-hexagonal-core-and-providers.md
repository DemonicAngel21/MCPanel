# ADR-0003: Hexagonal core with capability-segmented providers

- Status: Accepted
- Date: 2026-09-28

## Context

The UI must not control the system directly; external services and server software
families change frequently.

## Decision

`mcpanel-core` contains the domain, services and **ports** (traits). Adapters
(`mcpanel-db`, `mcpanel-providers`, `mcpanel-platform`) implement ports. The Application
API (`mcpanel-api`) is the only surface the UI uses. Server software is modelled with
several small traits (`ServerSoftware`, `SoftwareCatalog`, `SoftwareInstaller`,
`LaunchResolver`, `SoftwareDetector`) that return plans and specs as data; the core
executes them.

## Consequences

Providers are unit-testable without I/O and cannot spawn processes on their own. Adding
a provider means implementing traits and registering it.
