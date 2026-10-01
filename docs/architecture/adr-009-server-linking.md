# ADR 009: Minecraft Server-Link Identity Architecture

## Status

Accepted

## Context

MCPanel supports running local servers as well as orchestrating remote nodes (multihost). In future releases, users will be able to install an official MCPanel Minecraft server plugin (for Paper, Spigot, Fabric, Sponge, etc.) on remote or hosted Minecraft servers and link them directly to their MCPanel account.

A naive approach would pass user authentication tokens (e.g. Microsoft/Google OAuth access tokens or Firebase tokens) to the Minecraft server plugin. This would violate security boundaries:
1. Minecraft server owners or hostile server admins could extract the user's Microsoft/Google tokens and impersonate the user across other services.
2. If a server is compromised, user credentials and cloud access would be leaked.
3. Users would not be able to easily revoke access to a specific server without invalidating their entire account login.
4. Multiple MCPanel desktop installations or cluster hosts managing the same server would conflict over user tokens.

## Decision

We establish an **independent Server-Link Identity and Credential Model**:

1. **Decoupled Server Identity**:
   - Every linked Minecraft server is assigned an independent UUID (`ServerLinkId`).
   - The server plugin never receives, stores, or transmits user OAuth tokens (Google, Microsoft) or Firebase refresh tokens.
   - Servers authenticate using a dedicated, revocable `ServerCredentialSecret` (HMAC-SHA256 / Ed25519 key pair) generated exclusively for that server instance.

2. **Secure Enrollment Flow**:
   - The user requests a pairing token in the MCPanel desktop application.
   - MCPanel generates a cryptographically random, single-use `ServerEnrollmentToken` (`mcp_link_...`) with a strict time-to-live (default: 15 minutes).
   - The server administrator runs `/mcpanel link <token>` on the Minecraft server console.
   - The plugin initiates a TLS handshake with the MCPanel Cloud Gateway using the token.
   - The Gateway validates the token, links the server to the owner's stable internal account ID (`AccountId`), issues a permanent server secret, and invalidates the enrollment token.

3. **Least Privilege & Scoped Permissions (`ServerScope`)**:
   - Access between MCPanel and the plugin is constrained by explicit capability scopes:
     - `ConsoleRead`: View console output stream.
     - `ConsoleWrite`: Send commands into the server console.
     - `MetricsRead`: Read CPU, RAM, TPS, MSPT, and player count telemetry.
     - `PlayersManage`: Kick, ban, op, or whitelist players.
     - `PowerControl`: Send restart, stop, or shutdown signals.
     - `FilesRead`: Inspect config files with secrets automatically redacted.
     - `BackupsTrigger`: Request remote server snapshot backups.

4. **Multi-Host Governance & Multi-Server Support**:
   - An MCPanel account can hold multiple linked servers.
   - Multiple authorized MCPanel desktop installations (or cluster nodes) belonging to the account can observe and manage the same linked server according to the granted scopes.

5. **Instant Revocation**:
   - Account owners can revoke a linked server at any time from MCPanel.
   - When revoked, the server key is permanently disabled, immediately terminating telemetry and command execution without affecting the user's primary account session.

## Consequences

- **Security**: Zero credential leakage between user identity (Microsoft/Google) and game server environments.
- **Resilience**: Server compromises are contained strictly to that specific server's scoped access.
- **Usability**: Pairing is simple (single command), while revocation is instantaneous.
