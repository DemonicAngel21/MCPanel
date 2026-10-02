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

