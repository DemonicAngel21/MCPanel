# ADR-0010: Connect to existing Google Compute Engine hosts

- Status: Accepted
- Date: 2026-11-02

## Context

The multihost UI previously stored arbitrary addresses and fabricated online status,
metrics, servers, and successful remote operations. MCPanel needs a truthful first step
for users who already run Minecraft servers on Google Compute Engine Linux VMs.

## Decision

- Connect to an existing VM using its Google Cloud project ID, zone, and instance name.
- Use a dedicated Google OAuth consent flow with the Compute Engine scope. Keep the refresh
  token in the platform secret store and refresh short-lived access tokens in the backend.
- Call the Compute Engine REST API directly for VM metadata and explicit start/stop actions.
- Require the corresponding Compute Engine IAM permissions for VM start and stop. Those
  actions control the VM only and do not manage Minecraft processes.
- Never interpolate arbitrary user commands into the remote shell. Validate all GCE
  identifiers before passing them as process arguments.
- Do not create or delete VMs, change firewall rules, or install an agent as part of
  linking. VM creation and remote Minecraft process management require separate work.

## Consequences

- Firebase configuration is not required for the Google host connection.
- Users authorize MCPanel through Google OAuth and need permission to inspect/start/stop
  the VM as needed. No local Google Cloud CLI or inbound SSH access is required.
- The implementation reports VM status and network address from Compute Engine. It does not deploy or
  control Minecraft servers on the VM.
