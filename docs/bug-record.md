# Bug Record

This document records functional bugs identified, diagnosed, and resolved during MCPanel development and quality assurance passes.

---

## Batch 1: Accounts, Onboarding, and Settings Sync

### Bug 1: Microsoft Sign-In State Never Auto-Detected by UI
- **Severity**: High (UX blocking)
- **Component**: Frontend Data Fetching / Account Query
- **File**: [`apps/desktop/src/lib/queries.ts`](file:///C:/MCPanel/apps/desktop/src/lib/queries.ts#L124-L128)
- **Symptom**: When a user initiated "Continue with Microsoft" and completed the browser authorization flow, the desktop UI remained in the waiting/spinner state indefinitely until the user manually refreshed or navigated away and back.
- **Root Cause**: The `useAccount` query configured `refetchInterval` to poll every 1.5s only when `googleState === "waiting"`. It omitted `microsoftState === "waiting"`.
- **Fix**: Updated `refetchInterval` to poll whenever either `googleState === "waiting"` or `microsoftState === "waiting"`.
- **Status**: Resolved & Verified.

---

### Bug 2: Onboarding Setup Step Renders Sign-In Forms in Guest Mode
- **Severity**: Medium (UX confusion)
- **Component**: Onboarding Flow / First-Time Setup
- **File**: [`apps/desktop/src/pages/onboarding.tsx`](file:///C:/MCPanel/apps/desktop/src/pages/onboarding.tsx#L94-L96)
- **Symptom**: During initial onboarding, if a user clicked "Continue as Guest", the Account step continued to render the sign-in and account creation forms instead of acknowledging guest status.
- **Root Cause**: `AccountStep` only checked `account.signedIn` to switch to `AccountSummary`. For guest accounts, `account.signedIn` is `false` while `account.isGuest` is `true`.
- **Fix**: Changed the condition to `account.configured && (account.signedIn || account.isGuest) ? <AccountSummary /> : <AccountForms onSignedIn={next} />`.
- **Status**: Resolved & Verified.

---

### Bug 3: Stale Input State in NumberSetting After External Updates
- **Severity**: Medium (State synchronization)
- **Component**: Settings Form Controls
- **File**: [`apps/desktop/src/pages/settings.tsx`](file:///C:/MCPanel/apps/desktop/src/pages/settings.tsx#L81-L97)
- **Symptom**: Numeric settings fields (e.g. console buffer lines, quit stop timeout) showed obsolete values when preferences changed externally (such as after cloud sync or background settings reload), and the Save button was disabled or sent stale values.
- **Root Cause**: `NumberSetting` initialized local text state via `useState(String(value))` without synchronizing when the incoming `value` prop changed.
- **Fix**: Added prop-change synchronization tracking `prev` value in render phase to reset local text whenever canonical `value` updates.
- **Status**: Resolved & Verified.

---

## Batch 2: OAuth Provider Scoping, Modal Isolation, and Stale Closures

### Bug 4: Verification Badges Displayed for Inherently Verified OAuth Providers
- **Severity**: Low / Cosmetic
- **Component**: Account Profile Summary
- **File**: [`apps/desktop/src/components/account-panel.tsx`](file:///C:/MCPanel/apps/desktop/src/components/account-panel.tsx#L601-L633)
- **Symptom**: Google and Microsoft authenticated accounts showed "Not verified" badges or "Resend verification" action buttons, even though third-party OAuth providers are pre-verified and do not have email verification flows in Firebase.
- **Root Cause**: `AccountSummary` evaluated `!p.emailVerified` globally for all accounts rather than scoping it strictly to `password` (email/password) provider accounts.
- **Fix**: Added `isEmailProvider = p.provider === "password"` guard; verification status and resend actions are only rendered for email/password accounts.
- **Status**: Resolved & Verified.

---

### Bug 5: Stale Form Credentials Leaked on Reopening OAuth Config Modal
- **Severity**: Medium (Credential hygiene / UX)
- **Component**: OAuth Configuration Modal
- **File**: [`apps/desktop/src/components/account-panel.tsx`](file:///C:/MCPanel/apps/desktop/src/components/account-panel.tsx#L89-L235)
- **Symptom**: Closing the OAuth configuration dialog without saving left dirty or half-entered client IDs and secrets in the form fields when reopened later.
- **Root Cause**: Modal input states were held at the `GoogleConfigDialog` outer level and remained mounted across open/close toggles.
- **Fix**: Extracted inner form content into `GoogleConfigContent` and mounted it conditionally (`{open && <GoogleConfigContent ... />}`), guaranteeing fresh state initialization on every modal open without violating React effect rules.
- **Status**: Resolved & Verified.

---

### Bug 6: Stale Closure in Cloud Storage Sign-In Polling Interval
- **Severity**: Low
- **Component**: Cloud Storage Linking Card
- **File**: [`apps/desktop/src/components/cloud-card.tsx`](file:///C:/MCPanel/apps/desktop/src/components/cloud-card.tsx#L19-L35)
- **Symptom**: If provider properties updated while browser sign-in polling was active, toast notifications on completion/failure could reference a stale `displayName`.
- **Root Cause**: `setInterval` inside `useEffect` captured `p.displayName` from the initial closure, while `flow` was the sole dependency.
- **Fix**: Stored `displayName` in a ref updated via `useEffect` (`displayNameRef.current = p.displayName`), ensuring interval callbacks always read the latest provider display name.
- **Status**: Resolved & Verified.

---

### Bug 7: Multihost Area Retaining Logged-in State After Account Sign-Out
- **Severity**: High (State synchronization / UX)
- **Component**: Multihost Management Page & Account State Synchronization
- **Files**: [`apps/desktop/src/pages/hosts.tsx`](file:///C:/MCPanel/apps/desktop/src/pages/hosts.tsx#L90-L96), [`apps/desktop/src/components/account-panel.tsx`](file:///C:/MCPanel/apps/desktop/src/components/account-panel.tsx#L576-L600)
- **Symptom**: When signing out of an account from the Account panel or Settings, navigating to the Multihost page still showed the user as logged in with active node management controls until a manual page refresh.
- **Root Cause**: `hosts.tsx` relied on `status?.signedIn` from the cached `useMultihostStatus` query (10s refetch interval). The sign-out mutation only updated `qk.account` and did not invalidate or reset `qk.multihost` cache.
- **Fix**:
  1. Connected `hosts.tsx` `signedIn` directly to `Boolean(account?.signedIn && status?.signedIn)`, making `account` query the immediate single source of truth.
  2. Updated `account-panel.tsx` sign-out and guest actions to immediately reset `qk.multihost` cache (`signedIn: false`, `userEmail: null`, removing remote nodes) and trigger background query invalidation.
- **Status**: Resolved & Verified.

---

## Batch 3: Offline Mode & Whitelist Logic, Theme Sync, Console Stdin, and Backup Invalidation

### Bug 8: Whitelist Misinterpreted as Disabled on Offline-Mode Servers
- **Severity**: High (Misleading security state)
- **Component**: Server Players Whitelist View & Core Live Properties
- **Files**: [`apps/desktop/src/pages/server/players.tsx`](file:///C:/MCPanel/apps/desktop/src/pages/server/players.tsx), [`crates/mcpanel-core/src/players/service.rs`](file:///C:/MCPanel/crates/mcpanel-core/src/players/service.rs)
- **Symptom**: On cracked/offline-mode servers (`online-mode=false`), the Players tab displayed a warning telling the user to "Enable the whitelist or online mode" even when the server whitelist was already active (`white-list=true`).
- **Root Cause**: The UI conditionally displayed the warning purely based on `!data.onlineMode` without checking `!data.whitelistEnabled`. In addition, changes made via live console commands (`whitelist on`/`off`) were not immediately synchronized to `server.properties` on disk.
- **Fix**:
  1. Updated the Players banner logic: `!onlineMode && !whitelistEnabled` displays a warning that whitelist is disabled; `!onlineMode && whitelistEnabled` displays an informative banner that whitelist enforcement is active with offline mode.
  2. Updated `crates/mcpanel-core/src/players/service.rs` to persist `white-list` changes to `server.properties` immediately when applying live actions.
  3. Added comprehensive regression tests in [`apps/desktop/src/pages/server/players.test.tsx`](file:///C:/MCPanel/apps/desktop/src/pages/server/players.test.tsx).
- **Status**: Resolved & Verified.

---

### Bug 9: Monaco Editor Theme Desynchronization on App Theme Change
- **Severity**: Medium (Visual / UX)
- **Component**: Server File Editor
- **File**: [`apps/desktop/src/pages/server/editor.tsx`](file:///C:/MCPanel/apps/desktop/src/pages/server/editor.tsx)
- **Symptom**: When toggling between dark and light themes (or using the Ctrl+K palette command), the Monaco code editor remained permanently locked in the initial theme it was mounted with (e.g. dark editor on light background).
- **Root Cause**: The editor component read `document.documentElement.dataset.theme` once on mount without observing DOM attribute mutations on `data-theme` or notifying Monaco via `monaco.editor.setTheme`.
- **Fix**: Added a `MutationObserver` on `document.documentElement` watching `data-theme` that updates local reactive `theme` state and explicitly calls `monaco.editor.setTheme(next === "light" ? "vs" : "vs-dark")`.
- **Status**: Resolved & Verified.

---

### Bug 10: Server Console Commands With Leading Slash Fail on Dedicated Server Stdin
- **Severity**: Medium (Command execution failure)
- **Component**: Server Runtime Console Manager
- **File**: [`crates/mcpanel-core/src/server/manager.rs`](file:///C:/MCPanel/crates/mcpanel-core/src/server/manager.rs#L863-L873)
- **Symptom**: When server operators typed commands with a leading slash in the MCPanel console (e.g. `/say hello`, `/whitelist add Steve`, `/stop`), Minecraft dedicated server printed `"Unknown or incomplete command"` errors.
- **Root Cause**: The server console stdin reader expects commands without a leading slash (slashes are only parsed by the client chat protocol). `send_command` stripped the slash only for internal stop detection, but passed the raw command with the slash directly into the server process stdin.
- **Fix**: Stripped the leading slash before forwarding the command string to process stdin (`tx.send(bare.to_string())`) while preserving the full command in the console history log.
- **Status**: Resolved & Verified.

---

### Bug 11: Stale Deleted or Restored Backups in BackupList Due to Missing Query Invalidation
- **Severity**: Low / Medium (State synchronization)
- **Component**: Backup Table & Restore Modal
- **File**: [`apps/desktop/src/components/backup-list.tsx`](file:///C:/MCPanel/apps/desktop/src/components/backup-list.tsx)
- **Symptom**: Deleting a backup in the backups table or restoring a backup left stale rows in the table until the user manually refreshed or navigated away from the route.
- **Root Cause**: `BackupList` and `RestoreDialog` executed `api.backups.delete` and `api.backups.restore` without invalidating the `["backups"]` TanStack Query cache.
- **Fix**: Added `useQueryClient` and called `await qc.invalidateQueries({ queryKey: ["backups"] })` upon successful deletion and restoration.
- **Status**: Resolved & Verified.

