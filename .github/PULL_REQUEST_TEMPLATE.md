## What and why

<!-- What does this change and why? Link issues. -->

## Checklist

- [ ] Follows the architecture (UI → API → Core → adapters); no business logic in the UI
- [ ] No secrets, credentials, keys or Minecraft server data added
- [ ] Tests added/updated; `cargo test --workspace` and `pnpm check` pass
- [ ] Database changes are new migration files
- [ ] `CHANGELOG.md` updated under `[Unreleased]`
- [ ] External APIs verified and recorded in `docs/architecture/verification-log.md`

## Security considerations

<!-- File access, processes, network, secrets, untrusted input? -->
