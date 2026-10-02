import { useQueryClient } from "@tanstack/react-query";
import { BadgeCheck, Cloud, ExternalLink, KeyRound, LogOut, Mail, RefreshCw, Settings, User } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import type { AccountDto } from "@/bindings/AccountDto";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/overlays";
import { Badge, Field, Input, Spinner } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, useAccount } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";

/** Google's "G" mark (brand colors, as Google's sign-in guidelines ask). */
function GoogleMark() {
  return (
    <svg viewBox="0 0 48 48" aria-hidden className="size-4 shrink-0">
      <path
        fill="#FFC107"
        d="M43.6 20.5H42V20H24v8h11.3C33.7 32.7 29.2 36 24 36c-6.6 0-12-5.4-12-12s5.4-12 12-12c3.1 0 5.8 1.2 7.9 3.1l5.7-5.7C34 6.1 29.3 4 24 4 12.9 4 4 12.9 4 24s8.9 20 20 20 20-8.9 20-20c0-1.3-.1-2.4-.4-3.5z"
      />
      <path fill="#FF3D00" d="M6.3 14.7l6.6 4.8C14.7 15.1 19 12 24 12c3.1 0 5.8 1.2 7.9 3.1l5.7-5.7C34 6.1 29.3 4 24 4 16.3 4 9.7 8.3 6.3 14.7z" />
      <path fill="#4CAF50" d="M24 44c5.2 0 9.9-2 13.4-5.2l-6.2-5.2C29.2 35.1 26.7 36 24 36c-5.2 0-9.6-3.3-11.3-7.9l-6.5 5C9.5 39.6 16.2 44 24 44z" />
      <path fill="#1976D2" d="M43.6 20.5H42V20H24v8h11.3c-.8 2.3-2.2 4.2-4.1 5.6l6.2 5.2C37 39.2 44 34 44 24c0-1.3-.1-2.4-.4-3.5z" />
    </svg>
  );
}

/** Temporary feature flag to block Microsoft authentication until Azure account is configured */
export const ENABLE_MICROSOFT_AUTH = false;

/** Microsoft 4-square brand mark */
function MicrosoftMark() {
  return (
    <svg viewBox="0 0 21 21" aria-hidden className="size-4 shrink-0">
      <rect x="1" y="1" width="9" height="9" fill="#F25022" />
      <rect x="11" y="1" width="9" height="9" fill="#7FBA00" />
      <rect x="1" y="11" width="9" height="9" fill="#00A4EF" />
      <rect x="11" y="11" width="9" height="9" fill="#FFB900" />
    </svg>
  );
}

export function Avatar({ account, size = "md" }: { account: AccountDto; size?: "sm" | "md" }) {
  const p = account.profile;
  const initial = account.isGuest ? "G" : (p?.displayName ?? p?.email ?? "?").trim().charAt(0).toUpperCase();
  const cls = size === "sm" ? "size-7 text-xs" : "size-10 text-sm";
  const [broken, setBroken] = useState(false);
  return p?.photoUrl && !broken ? (
    <img src={p.photoUrl} alt="" referrerPolicy="no-referrer" onError={() => setBroken(true)} className={cn("rounded-full object-cover", cls)} />
  ) : (
    <span aria-hidden className={cn("flex items-center justify-center rounded-full bg-accent font-semibold text-accent-fg", cls)}>
      {initial}
    </span>
  );
}

/** Report the end of a Google sign-in started here, once. */
function useGoogleOutcome(account: AccountDto | undefined, onDone?: () => void) {
  const seen = useRef<string | null>(null);
  useEffect(() => {
    const st = account?.googleState;
    if (!st || st === seen.current) return;
    const first = seen.current === null && st !== "waiting";
    seen.current = st;
    if (first) return;
    if (st === "done") {
      toast.success("Signed in with Google");
      onDone?.();
    }
    if (st === "failed" && account?.googleError) toast.error(account.googleError);
  }, [account?.googleState, account?.googleError, onDone]);
}

/** Report the end of a Microsoft sign-in started here, once. */
function useMicrosoftOutcome(account: AccountDto | undefined, onDone?: () => void) {
  const seen = useRef<string | null>(null);
  useEffect(() => {
    const st = account?.microsoftState;
    if (!st || st === seen.current) return;
    const first = seen.current === null && st !== "waiting";
    seen.current = st;
    if (first) return;
    if (st === "done") {
      toast.success("Signed in with Microsoft");
      onDone?.();
    }
    if (st === "failed" && account?.microsoftError) toast.error(account.microsoftError);
  }, [account?.microsoftState, account?.microsoftError, onDone]);
}

function GoogleConfigContent({ onOpenChange, onSaved }: { onOpenChange: (open: boolean) => void; onSaved?: () => void }) {
  const qc = useQueryClient();
  const { data: account } = useAccount();
  const [googleClientId, setGoogleClientId] = useState("");
  const [googleClientSecret, setGoogleClientSecret] = useState("");
  const [msClientId, setMsClientId] = useState("");
  const [msClientSecret, setMsClientSecret] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setBusy(true);
    try {
      const res = await api.account.configureOauth({
        googleClientId: googleClientId.trim() || null,
        googleClientSecret: googleClientSecret.trim() || null,
        microsoftClientId: msClientId.trim() || null,
        microsoftClientSecret: msClientSecret.trim() || null,
        firebaseApiKey: apiKey.trim() || null,
      });
      qc.setQueryData(qk.account, res);
      toast.success("OAuth credentials updated");
      onOpenChange(false);
      onSaved?.();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogContent
      title="OAuth & Cloud Provider Setup"
      description={
        ENABLE_MICROSOFT_AUTH
          ? "Configure desktop OAuth Client IDs for Google or Microsoft sign-in."
          : "Configure desktop OAuth Client IDs for Google sign-in."
      }
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => void save()} disabled={busy}>
            {busy && <Spinner className="text-accent-fg" />} Save Credentials
          </Button>
        </>
      }
    >
      <div className="space-y-4 text-xs">
        <div className="rounded-md border border-border bg-surface-2 p-3 text-muted">
          <p className="leading-relaxed">
            In production releases, credentials are built-in so users sign in with 1 click. In custom or development builds, you can provide client
            IDs from{" "}
            <a
              href="https://console.cloud.google.com/apis/credentials"
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-0.5 text-accent underline"
            >
              Google Cloud Console <ExternalLink className="inline size-3" />
            </a>
            {ENABLE_MICROSOFT_AUTH && (
              <>
                {" "}
                or{" "}
                <a
                  href="https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade"
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-0.5 text-accent underline"
                >
                  Microsoft Entra ID <ExternalLink className="inline size-3" />
                </a>
              </>
            )}
            .
          </p>
          {account?.customOauthConfigured && <p className="mt-2 font-medium text-accent">Custom credentials are configured.</p>}
        </div>

        <div className="space-y-3">
          <h4 className="font-semibold text-fg">Google OAuth</h4>
          <Field label="Google Client ID" hint="Desktop app type">
            <Input
              placeholder="e.g. 123456789-xxxxxx.apps.googleusercontent.com"
              value={googleClientId}
              onChange={(e) => setGoogleClientId(e.target.value)}
            />
          </Field>

          <Field label="Google Client Secret" hint="Optional for Desktop apps">
            <Input
              type="password"
              placeholder="e.g. GOCSPX-xxxxxxxxxxxxxxxx"
              value={googleClientSecret}
              onChange={(e) => setGoogleClientSecret(e.target.value)}
            />
          </Field>
        </div>

        {ENABLE_MICROSOFT_AUTH && (
          <div className="space-y-3 border-t border-border pt-3">
            <h4 className="font-semibold text-fg">Microsoft OAuth</h4>
            <Field label="Microsoft Client ID" hint="Application (client) ID from Azure">
              <Input placeholder="e.g. 00000000-0000-0000-0000-000000000000" value={msClientId} onChange={(e) => setMsClientId(e.target.value)} />
            </Field>

            <Field label="Microsoft Client Secret" hint="Optional for public desktop clients">
              <Input
                type="password"
                placeholder="Optional client secret"
                value={msClientSecret}
                onChange={(e) => setMsClientSecret(e.target.value)}
              />
            </Field>
          </div>
        )}

        <div>
          <button type="button" onClick={() => setShowAdvanced(!showAdvanced)} className="cursor-pointer text-xs text-muted underline hover:text-fg">
            {showAdvanced ? "Hide Advanced Options" : "Show Advanced Options (Custom Firebase)"}
          </button>
          {showAdvanced && (
            <div className="mt-3 space-y-3 border-t border-border pt-3">
              <Field label="Firebase Web API Key" hint="Optional custom Firebase project">
                <Input placeholder="e.g. AIzaSy..." value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
              </Field>
            </div>
          )}
        </div>
      </div>
    </DialogContent>
  );
}

/** Dialog to configure Google, Microsoft, and Firebase OAuth credentials */
export function GoogleConfigDialog({ open, onOpenChange, onSaved }: { open: boolean; onOpenChange: (open: boolean) => void; onSaved?: () => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open && <GoogleConfigContent onOpenChange={onOpenChange} onSaved={onSaved} />}
    </Dialog>
  );
}

/** Sign in or create an account (email, Google, Microsoft, or Guest). */
export function AccountForms({
  onSignedIn,
  initialMode = "signin",
  onOpenConfig,
}: {
  onSignedIn?: () => void;
  initialMode?: "create" | "signin";
  onOpenConfig?: () => void;
}) {
  const qc = useQueryClient();
  const { data: account } = useAccount();
  const [mode, setMode] = useState<"create" | "signin">(initialMode);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState<null | "form" | "google" | "microsoft" | "guest" | "reset">(null);
  const [error, setError] = useState<string | null>(null);
  const [configOpen, setConfigOpen] = useState(false);
  const [pendingLaunch, setPendingLaunch] = useState<null | "google" | "microsoft">(null);

  useGoogleOutcome(account, onSignedIn);
  useMicrosoftOutcome(account, onSignedIn);

  if (!account) return <Spinner />;
  if (!account.configured) {
    return (
      <>
        <AccountUnavailable onConfigure={onOpenConfig ?? (() => setConfigOpen(true))} />
        <GoogleConfigDialog
          open={configOpen}
          onOpenChange={setConfigOpen}
          onSaved={() => {
            if (pendingLaunch === "google") {
              setPendingLaunch(null);
              void google();
            } else if (pendingLaunch === "microsoft") {
              setPendingLaunch(null);
              void microsoft();
            }
          }}
        />
      </>
    );
  }

  const waitingGoogle = account.googleState === "waiting";
  const waitingMicrosoft = account.microsoftState === "waiting";

  const submit = async () => {
    setBusy("form");
    setError(null);
    try {
      const r = mode === "create" ? await api.account.signUp(email, password, name.trim() || null) : await api.account.signIn(email, password);
      qc.setQueryData(qk.account, r);
      void qc.invalidateQueries({ queryKey: qk.multihost });
      void qc.invalidateQueries({ queryKey: qk.settings });
      toast.success(mode === "create" ? "Account created. Verification email sent." : "Signed in");
      onSignedIn?.();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const google = async () => {
    setBusy("google");
    setError(null);
    try {
      await api.account.google();
      await qc.invalidateQueries({ queryKey: qk.account });
      void qc.invalidateQueries({ queryKey: qk.multihost });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const microsoft = async () => {
    setBusy("microsoft");
    setError(null);
    try {
      await api.account.microsoft();
      await qc.invalidateQueries({ queryKey: qk.account });
      void qc.invalidateQueries({ queryKey: qk.multihost });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const guest = async () => {
    setBusy("guest");
    setError(null);
    try {
      const r = await api.account.guestSignIn();
      qc.setQueryData(qk.account, r);
      qc.setQueryData(qk.multihost, (prev: unknown) =>
        prev && typeof prev === "object"
          ? {
              ...(prev as object),
              signedIn: false,
              userEmail: null,
              hosts: Array.isArray((prev as { hosts?: unknown[] }).hosts)
                ? (prev as { hosts: Array<{ isLocal?: boolean }> }).hosts.filter((h) => h.isLocal)
                : [],
            }
          : prev,
      );
      void qc.invalidateQueries({ queryKey: qk.multihost });
      toast.success("Continuing as Guest");
      onSignedIn?.();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const handleGoogleClick = async () => {
    if (!account.googleAvailable) {
      setPendingLaunch("google");
      if (onOpenConfig) onOpenConfig();
      else setConfigOpen(true);
      return;
    }
    await google();
  };

  const handleMicrosoftClick = async () => {
    if (!account.microsoftAvailable) {
      setPendingLaunch("microsoft");
      if (onOpenConfig) onOpenConfig();
      else setConfigOpen(true);
      return;
    }
    await microsoft();
  };

  const reset = async () => {
    setBusy("reset");
    setError(null);
    try {
      await api.account.resetPassword(email);
      toast.success("If an account exists for this email, a reset link was sent.");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-4">
      {/* Waiting banner for Google redirect */}
      {waitingGoogle && (
        <div className="flex items-center justify-between gap-2 rounded-md bg-info-soft px-3 py-2.5 text-xs text-fg">
          <span className="flex items-center gap-2">
            <Spinner /> Complete Google sign-in in your browser…
          </span>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void api.account.cancelGoogle().then(() => qc.invalidateQueries({ queryKey: qk.account }))}
          >
            Cancel
          </Button>
        </div>
      )}

      {/* Waiting banner for Microsoft redirect */}
      {ENABLE_MICROSOFT_AUTH && waitingMicrosoft && (
        <div className="flex items-center justify-between gap-2 rounded-md bg-info-soft px-3 py-2.5 text-xs text-fg">
          <span className="flex items-center gap-2">
            <Spinner /> Complete Microsoft sign-in in your browser…
          </span>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void api.account.cancelMicrosoft().then(() => qc.invalidateQueries({ queryKey: qk.account }))}
          >
            Cancel
          </Button>
        </div>
      )}

      {/* OAuth & Guest Options */}
      {!waitingGoogle && (!ENABLE_MICROSOFT_AUTH || !waitingMicrosoft) && (
        <div className="grid gap-2">
          <Button variant="outline" className="w-full" onClick={() => void handleGoogleClick()} disabled={busy != null}>
            {busy === "google" ? <Spinner /> : <GoogleMark />} Continue with Google
          </Button>

          {ENABLE_MICROSOFT_AUTH && (
            <Button variant="outline" className="w-full" onClick={() => void handleMicrosoftClick()} disabled={busy != null}>
              {busy === "microsoft" ? <Spinner /> : <MicrosoftMark />} Continue with Microsoft
            </Button>
          )}

          <Button variant="ghost" className="w-full text-muted hover:text-fg" onClick={() => void guest()} disabled={busy != null}>
            {busy === "guest" ? <Spinner /> : <User className="size-4" />} Continue as Guest (Local Only)
          </Button>
        </div>
      )}

      <div className="flex items-center gap-3 text-[11px] text-faint uppercase" aria-hidden>
        <span className="h-px flex-1 bg-border" /> or with email <span className="h-px flex-1 bg-border" />
      </div>

      <div role="tablist" aria-label="Account" className="grid grid-cols-2 rounded-md border border-border p-0.5">
        {(["create", "signin"] as const).map((m) => (
          <button
            key={m}
            role="tab"
            aria-selected={mode === m}
            onClick={() => {
              setMode(m);
              setError(null);
            }}
            className={cn(
              "cursor-default rounded px-2 py-1 text-xs transition-colors duration-150",
              mode === m ? "bg-surface-3 font-medium text-fg" : "text-muted hover:text-fg",
            )}
          >
            {m === "create" ? "Create account" : "Sign in"}
          </button>
        ))}
      </div>

      <form
        className="space-y-3"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        {mode === "create" && (
          <Field label="Name (optional)">
            <Input value={name} maxLength={64} autoComplete="nickname" onChange={(e) => setName(e.target.value)} />
          </Field>
        )}
        <Field label="Email">
          <Input type="email" value={email} autoComplete="email" onChange={(e) => setEmail(e.target.value)} />
        </Field>
        <Field label="Password" hint={mode === "create" ? "At least 8 characters." : undefined}>
          <Input
            type="password"
            value={password}
            autoComplete={mode === "create" ? "new-password" : "current-password"}
            onChange={(e) => setPassword(e.target.value)}
          />
        </Field>
        {error && (
          <p role="alert" className="text-xs text-danger">
            {error}
          </p>
        )}
        <div className="flex flex-wrap items-center justify-between gap-2">
          {mode === "signin" ? (
            <button type="button" className="cursor-default text-xs text-accent hover:underline" onClick={() => void reset()} disabled={busy != null}>
              Forgot password?
            </button>
          ) : (
            <span className="text-[11px] text-faint">Passwords are processed securely and never stored locally.</span>
          )}
          <Button variant="primary" type="submit" disabled={busy != null || !email || !password}>
            {busy === "form" && <Spinner className="text-accent-fg" />} {mode === "create" ? "Create account" : "Sign in"}
          </Button>
        </div>
      </form>

      <div className="flex items-center justify-between border-t border-border pt-2 text-[11px] text-muted">
        <span>{account.googleAvailable || account.microsoftAvailable ? "OAuth providers active" : "OAuth configuration"}</span>
        <button
          type="button"
          onClick={() => (onOpenConfig ? onOpenConfig() : setConfigOpen(true))}
          className="inline-flex cursor-pointer items-center gap-1 text-accent hover:underline"
        >
          <KeyRound className="size-3" /> {account.customOauthConfigured ? "Manage OAuth keys" : "Setup OAuth"}
        </button>
      </div>

      <GoogleConfigDialog
        open={configOpen}
        onOpenChange={setConfigOpen}
        onSaved={() => {
          if (pendingLaunch === "google") {
            setPendingLaunch(null);
            void google();
          } else if (pendingLaunch === "microsoft") {
            setPendingLaunch(null);
            void microsoft();
          }
        }}
      />
    </div>
  );
}

export function AccountUnavailable({ onConfigure }: { onConfigure?: () => void }) {
  const qc = useQueryClient();
  const enterGuest = async () => {
    try {
      const res = await api.account.guestSignIn();
      qc.setQueryData(qk.account, res);
      qc.setQueryData(qk.multihost, (prev: unknown) =>
        prev && typeof prev === "object"
          ? {
              ...(prev as object),
              signedIn: false,
              userEmail: null,
              hosts: Array.isArray((prev as { hosts?: unknown[] }).hosts)
                ? (prev as { hosts: Array<{ isLocal?: boolean }> }).hosts.filter((h) => h.isLocal)
                : [],
            }
          : prev,
      );
      void qc.invalidateQueries({ queryKey: qk.multihost });
      toast.success("Guest mode enabled");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <div className="space-y-3 rounded-md border border-border bg-surface-2 p-3 text-xs text-muted">
      <p>
        Accounts are not available in this build of MCPanel (no Firebase project is configured). You can use MCPanel locally as a guest, or configure
        custom credentials.
      </p>
      <div className="flex flex-wrap gap-2">
        <Button variant="primary" size="sm" onClick={() => void enterGuest()}>
          <User className="size-3.5" /> Continue as Guest
        </Button>
        {onConfigure && (
          <Button variant="outline" size="sm" onClick={onConfigure}>
            <KeyRound className="size-3.5" /> Configure OAuth Keys
          </Button>
        )}
      </div>
    </div>
  );
}

/** The active account (or Guest): picture, name, email, sync, and sign-out. */
export function AccountSummary({ onUpgrade }: { onUpgrade?: () => void }) {
  const qc = useQueryClient();
  const { data: account } = useAccount();
  const [busy, setBusy] = useState<string | null>(null);

  if (!account) return null;

  const run = async (kind: string, fn: () => Promise<unknown>, ok?: string) => {
    setBusy(kind);
    try {
      const r = await fn();
      if (r && typeof r === "object") qc.setQueryData(qk.account, r);
      if (kind === "out") {
        qc.setQueryData(qk.multihost, (prev: unknown) =>
          prev && typeof prev === "object"
            ? {
                ...(prev as object),
                signedIn: false,
                userEmail: null,
                hosts: Array.isArray((prev as { hosts?: unknown[] }).hosts)
                  ? (prev as { hosts: Array<{ isLocal?: boolean }> }).hosts.filter((h) => h.isLocal)
                  : [],
              }
            : prev,
        );
        void qc.invalidateQueries({ queryKey: qk.multihost });
        void qc.invalidateQueries({ queryKey: qk.settings });
      }
      if (ok) toast.success(ok);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  // Guest Mode display
  if (account.isGuest) {
    return (
      <div className="space-y-3 rounded-lg border border-border bg-surface-2 p-4">
        <div className="flex items-center gap-3">
          <span aria-hidden className="flex size-10 items-center justify-center rounded-full bg-surface-3 font-semibold text-fg">
            G
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <p className="truncate text-sm font-medium text-fg">Guest User</p>
              <Badge tone="neutral">Guest Mode</Badge>
            </div>
            <p className="text-xs text-muted">Local servers operate without restriction. Sign in to sync settings and manage remote nodes.</p>
          </div>
        </div>
        <div className="flex flex-wrap gap-2 pt-1">
          {onUpgrade && (
            <Button size="sm" variant="primary" onClick={onUpgrade}>
              Sign In or Create Account
            </Button>
          )}
          <Button size="sm" variant="ghost" onClick={() => run("out", api.account.signOut, "Exited guest mode")} disabled={busy != null}>
            <LogOut /> Exit Guest Mode
          </Button>
        </div>
      </div>
    );
  }

  if (!account.signedIn || !account.profile) return null;
  const p = account.profile;

  const providerLabel = p.provider === "google.com" ? "Google" : p.provider === "microsoft.com" ? "Microsoft" : "Email";
  const isEmailProvider = p.provider === "password";

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <Avatar account={account} />
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium text-fg">{p.displayName ?? p.email ?? "Your account"}</p>
          <p className="flex items-center gap-1.5 truncate text-xs text-muted">
            {p.email}
            {isEmailProvider &&
              (p.emailVerified ? (
                <Badge tone="success">
                  <BadgeCheck className="size-3" /> Verified
                </Badge>
              ) : (
                <Badge tone="warning">Not verified</Badge>
              ))}
            <Badge tone="neutral">{providerLabel}</Badge>
          </p>
        </div>
      </div>
      <div className="flex flex-wrap gap-1.5">
        <Button
          size="sm"
          variant="outline"
          onClick={() => run("sync", api.account.syncSettings, "Cloud settings synchronized")}
          disabled={busy != null}
          title="Sync theme, accent and preferences with the cloud"
        >
          {busy === "sync" ? <Spinner /> : <Cloud className="size-3.5" />} Sync Settings
        </Button>
        {isEmailProvider && !p.emailVerified && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => run("resend", api.account.resendVerification, "Verification email sent")}
            disabled={busy != null}
          >
            <Mail /> Resend verification
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={() => run("refresh", api.account.refresh)} disabled={busy != null}>
          {busy === "refresh" ? <Spinner /> : <RefreshCw />} Refresh
        </Button>
        <Button size="sm" variant="ghost" onClick={() => run("out", api.account.signOut, "Signed out")} disabled={busy != null}>
          <LogOut /> Sign out
        </Button>
      </div>
    </div>
  );
}

/** Account card content for Settings. */
export function AccountPanel() {
  const { data: account } = useAccount();
  const [configOpen, setConfigOpen] = useState(false);
  const [upgrading, setUpgrading] = useState(false);

  if (!account) return <Spinner />;

  if (!account.configured) {
    return (
      <>
        <AccountUnavailable onConfigure={() => setConfigOpen(true)} />
        <GoogleConfigDialog open={configOpen} onOpenChange={setConfigOpen} />
      </>
    );
  }

  if (account.isGuest && upgrading) {
    return (
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-medium text-fg">Upgrade to Full Account</h4>
          <Button variant="ghost" size="sm" onClick={() => setUpgrading(false)}>
            Back to Guest Profile
          </Button>
        </div>
        <AccountForms onSignedIn={() => setUpgrading(false)} initialMode="signin" onOpenConfig={() => setConfigOpen(true)} />
        <GoogleConfigDialog open={configOpen} onOpenChange={setConfigOpen} />
      </div>
    );
  }

  if (account.signedIn || account.isGuest) {
    return (
      <div className="space-y-4">
        <AccountSummary onUpgrade={() => setUpgrading(true)} />
        <div className="flex items-center justify-between border-t border-border pt-3">
          <span className="text-xs text-muted">OAuth & Cloud Settings</span>
          <Button variant="ghost" size="sm" onClick={() => setConfigOpen(true)}>
            <Settings className="size-3.5" /> Configure Providers
          </Button>
        </div>
        <GoogleConfigDialog open={configOpen} onOpenChange={setConfigOpen} />
      </div>
    );
  }

  return (
    <>
      <AccountForms initialMode="signin" onOpenConfig={() => setConfigOpen(true)} />
      <GoogleConfigDialog open={configOpen} onOpenChange={setConfigOpen} />
    </>
  );
}
