import { useQueryClient } from "@tanstack/react-query";
import { BadgeCheck, LogOut, Mail, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import type { AccountDto } from "@/bindings/AccountDto";
import { Button } from "@/components/ui/button";
import { Badge, Field, Input, Spinner } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, useAccount } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";

/** Google's "G" mark (brand colors, as Google's sign-in guidelines ask). */
function GoogleMark() {
  return (
    <svg viewBox="0 0 48 48" aria-hidden className="size-4">
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

export function Avatar({ account, size = "md" }: { account: AccountDto; size?: "sm" | "md" }) {
  const p = account.profile;
  const initial = (p?.displayName ?? p?.email ?? "?").trim().charAt(0).toUpperCase();
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

/** Sign in or create an account (email and password, or Google). */
export function AccountForms({ onSignedIn, initialMode = "create" }: { onSignedIn?: () => void; initialMode?: "create" | "signin" }) {
  const qc = useQueryClient();
  const { data: account } = useAccount();
  const [mode, setMode] = useState<"create" | "signin">(initialMode);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState<null | "form" | "google" | "reset">(null);
  const [error, setError] = useState<string | null>(null);
  useGoogleOutcome(account, onSignedIn);

  if (!account) return <Spinner />;
  if (!account.configured) {
    return (
      <p className="rounded-md border border-border bg-surface-2 px-3 py-2.5 text-xs text-muted">
        Accounts are not available in this build of MCPanel (no Firebase project is configured). You can use MCPanel without an account.
      </p>
    );
  }
  const waitingGoogle = account.googleState === "waiting";

  const submit = async () => {
    setBusy("form");
    setError(null);
    try {
      const r = mode === "create" ? await api.account.signUp(email, password, name.trim() || null) : await api.account.signIn(email, password);
      qc.setQueryData(qk.account, r);
      toast.success(mode === "create" ? "Account created. Check your email to verify it." : "Signed in");
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
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  const reset = async () => {
    setBusy("reset");
    setError(null);
    try {
      await api.account.resetPassword(email);
      toast.success("If an account exists for this email, a reset link is on its way.");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-4">
      {account.googleAvailable &&
        (waitingGoogle ? (
          <div className="flex items-center justify-between gap-2 rounded-md bg-info-soft px-3 py-2.5 text-xs text-fg">
            <span className="flex items-center gap-2">
              <Spinner /> Finish signing in with Google in your browser…
            </span>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void api.account.cancelGoogle().then(() => qc.invalidateQueries({ queryKey: qk.account }))}
            >
              Cancel
            </Button>
          </div>
        ) : (
          <Button variant="outline" className="w-full" onClick={() => void google()} disabled={busy != null}>
            {busy === "google" ? <Spinner /> : <GoogleMark />} Continue with Google
          </Button>
        ))}
      {account.googleAvailable && (
        <div className="flex items-center gap-3 text-[11px] text-faint uppercase" aria-hidden>
          <span className="h-px flex-1 bg-border" /> or with email <span className="h-px flex-1 bg-border" />
        </div>
      )}
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
            <button
              type="button"
              className="cursor-default text-xs text-accent-text hover:underline"
              onClick={() => void reset()}
              disabled={busy != null}
            >
              Forgot password?
            </button>
          ) : (
            <span className="text-[11px] text-faint">Your password is sent securely to Firebase and never stored by MCPanel.</span>
          )}
          <Button variant="primary" type="submit" disabled={busy != null || !email || !password}>
            {busy === "form" && <Spinner className="text-accent-fg" />} {mode === "create" ? "Create account" : "Sign in"}
          </Button>
        </div>
      </form>
    </div>
  );
}

/** The signed-in account: picture, name, email, verification and sign-out. */
export function AccountSummary() {
  const qc = useQueryClient();
  const { data: account } = useAccount();
  const [busy, setBusy] = useState<string | null>(null);
  if (!account?.signedIn || !account.profile) return null;
  const p = account.profile;
  const run = async (kind: string, fn: () => Promise<unknown>, ok?: string) => {
    setBusy(kind);
    try {
      const r = await fn();
      if (r && typeof r === "object") qc.setQueryData(qk.account, r);
      if (ok) toast.success(ok);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <Avatar account={account} />
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium text-fg">{p.displayName ?? p.email ?? "Your account"}</p>
          <p className="flex items-center gap-1.5 truncate text-xs text-muted">
            {p.email}
            {p.emailVerified ? (
              <Badge tone="success">
                <BadgeCheck className="size-3" /> Verified
              </Badge>
            ) : (
              <Badge tone="warning">Not verified</Badge>
            )}
            <Badge tone="neutral">{p.provider === "google.com" ? "Google" : "Email"}</Badge>
          </p>
        </div>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {!p.emailVerified && (
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
  if (!account) return <Spinner />;
  return account.signedIn ? <AccountSummary /> : <AccountForms initialMode="signin" />;
}
