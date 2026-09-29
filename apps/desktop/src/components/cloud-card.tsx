import { useQueryClient } from "@tanstack/react-query";
import { Cloud, Link2, RefreshCw, Unlink } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import type { CloudStatusDto } from "@/bindings/CloudStatusDto";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/overlays";
import { Badge, Card, CardHeader, Spinner, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatRelative } from "@/lib/format";
import { qk, useCloud } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

function ProviderRow({ p }: { p: CloudStatusDto }) {
  const qc = useQueryClient();
  const [flow, setFlow] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const timer = useRef<number | null>(null);
  const refresh = () => void qc.invalidateQueries({ queryKey: qk.cloud });

  // Follow the browser sign-in until it finishes.
  useEffect(() => {
    if (!flow) return;
    timer.current = window.setInterval(async () => {
      try {
        const f = await api.cloud.flow(flow);
        if (f.state === "waiting") return;
        setFlow(null);
        refresh();
        if (f.state === "connected") toast.success(`${p.displayName} connected`);
        else if (f.state === "failed") toast.error(f.message ?? `${p.displayName} sign-in failed`, { duration: 10_000 });
      } catch (e) {
        setFlow(null);
        toast.error(errorMessage(e));
      }
    }, 1000);
    return () => {
      if (timer.current) window.clearInterval(timer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flow]);

  const connect = async () => {
    try {
      setFlow(await api.cloud.connect(p.id));
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };
  const check = async () => {
    setBusy(true);
    try {
      await api.cloud.check(p.id);
      refresh();
      toast.success(`${p.displayName} connection works`);
    } catch (e) {
      toast.error(errorMessage(e), { duration: 10_000 });
    } finally {
      setBusy(false);
    }
  };
  const disconnect = async () => {
    try {
      const outcome = await api.cloud.disconnect(p.id);
      refresh();
      if (outcome === "revoked") toast.success(`${p.displayName} disconnected and access revoked`);
      else
        toast.message(`${p.displayName} disconnected on this computer`, {
          description: "Remove MCPanel's access in your account settings as well.",
          action: { label: "Open", onClick: () => void api.app.openExternal(p.manageAccessUrl).catch(() => {}) },
        });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const account = p.accountEmail ?? p.accountName;
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3">
      <div className="min-w-0">
        <p className="flex items-center gap-2 text-[13px] text-fg">
          {p.displayName}
          {!p.configured ? (
            <Tooltip content={`This build of MCPanel has no ${p.displayName} app registration yet (${p.clientIdVariable}).`}>
              <Badge tone="neutral">Not configured</Badge>
            </Tooltip>
          ) : p.connected ? (
            <Badge tone="success">Connected</Badge>
          ) : p.needsReconnect ? (
            <Badge tone="warning">Reconnect needed</Badge>
          ) : (
            <Badge tone="neutral">Not connected</Badge>
          )}
        </p>
        <p className="mt-0.5 truncate text-xs text-muted">
          {p.connected && account
            ? `${account}${p.connectedAt != null ? ` · connected ${formatRelative(p.connectedAt)}` : ""}`
            : !p.configured
              ? "Not available in this build."
              : p.needsReconnect
                ? "The sign-in is not stored on this computer. Connect again."
                : flow
                  ? "Waiting for you to finish signing in in your browser…"
                  : "Sign in with your account in the browser."}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {flow ? (
          <>
            <Spinner />
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                void api.cloud.cancel(flow);
              }}
            >
              Cancel
            </Button>
          </>
        ) : p.connected ? (
          <>
            <Tooltip content="Check the connection">
              <Button size="icon-sm" variant="ghost" aria-label={`Check ${p.displayName} connection`} onClick={check} disabled={busy}>
                {busy ? <Spinner /> : <RefreshCw />}
              </Button>
            </Tooltip>
            <Button size="sm" variant="ghost" onClick={() => setConfirm(true)}>
              <Unlink /> Disconnect
            </Button>
          </>
        ) : (
          <Button size="sm" variant="outline" onClick={connect} disabled={!p.configured}>
            <Link2 /> Connect
          </Button>
        )}
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={`Disconnect ${p.displayName}?`}
        description="MCPanel removes its stored sign-in and revokes access where the provider allows it. Files already in your cloud storage stay there."
        confirmLabel="Disconnect"
        destructive
        onConfirm={disconnect}
      />
    </div>
  );
}

/** Settings card: connect or disconnect cloud storage accounts. */
export function CloudCard() {
  const { data } = useCloud();
  if (!data) return null;
  return (
    <Card>
      <CardHeader
        title="Cloud storage"
        description="Link Google Drive, OneDrive or Dropbox. MCPanel signs in through your browser; your password never passes through MCPanel, and the sign-in is stored in the Windows Credential Manager."
      />
      <div className="divide-y divide-border">
        {data.map((p) => (
          <ProviderRow key={p.id} p={p} />
        ))}
      </div>
      <p className="flex items-center gap-2 border-t border-border px-4 py-2.5 text-[11px] text-faint">
        <Cloud className="size-3.5" /> Backups to cloud storage are not available yet — connecting only links the account.
      </p>
    </Card>
  );
}
