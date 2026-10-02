import { useQueryClient } from "@tanstack/react-query";
import { CloudDownload, CloudUpload, Download, HardDrive, Link2, RefreshCw, Trash2, Unlink } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import type { CloudFileDto } from "@/bindings/CloudFileDto";
import type { CloudStatusDto } from "@/bindings/CloudStatusDto";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/overlays";
import { Badge, Card, CardHeader, Spinner, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes, formatDateTime, formatRelative } from "@/lib/format";
import { qk, useCloud, useCloudFiles, useCloudOperations } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

function ProviderRow({ p }: { p: CloudStatusDto }) {
  const qc = useQueryClient();
  const [flow, setFlow] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const timer = useRef<number | null>(null);
  const displayNameRef = useRef(p.displayName);
  useEffect(() => {
    displayNameRef.current = p.displayName;
  }, [p.displayName]);
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: qk.cloud });
    void qc.invalidateQueries({ queryKey: qk.cloudFiles() });
  };

  // Follow the browser sign-in until it finishes.
  useEffect(() => {
    if (!flow) return;
    timer.current = window.setInterval(async () => {
      try {
        const f = await api.cloud.flow(flow);
        if (f.state === "waiting") return;
        setFlow(null);
        refresh();
        const displayName = displayNameRef.current;
        if (f.state === "connected") toast.success(`${displayName} connected`);
        else if (f.state === "failed") toast.error(f.message ?? `${displayName} sign-in failed`, { duration: 10_000 });
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
          {p.isGuest ? (
            <Badge tone="neutral">Sign-in required</Badge>
          ) : !p.configured && p.connected ? (
            <Tooltip
              content={`This build of MCPanel has no ${p.displayName} app registration (${p.clientIdVariable}), so it cannot use the linked account. You can still disconnect it.`}
            >
              <Badge tone="warning">Not usable in this build</Badge>
            </Tooltip>
          ) : !p.configured ? (
            <Tooltip content={`This build of MCPanel has no ${p.displayName} app registration (${p.clientIdVariable}).`}>
              <Badge tone="neutral">Not available in this build</Badge>
            </Tooltip>
          ) : p.connected && p.autoLinked ? (
            <Tooltip content="Automatically linked with your signed-in Google account">
              <Badge tone="success">Auto-linked</Badge>
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
          {p.isGuest
            ? "Sign in with an MCPanel account to link cloud backup."
            : p.connected && p.autoLinked
              ? `${account ? `${account} · ` : ""}automatically linked via your Google account.`
              : p.connected && account
                ? `${account}${p.connectedAt != null ? ` · connected ${formatRelative(p.connectedAt)}` : ""}`
                : !p.configured
                  ? "Not available in this build."
                  : p.needsReconnect
                    ? "The sign-in is not stored on this computer. Connect again."
                    : flow
                      ? "Waiting for you to finish signing in in your browser…"
                      : `Sign in with your ${p.displayName} account in the browser.`}
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
            {p.configured && (
              <Tooltip content="Check the connection">
                <Button size="icon-sm" variant="ghost" aria-label={`Check ${p.displayName} connection`} onClick={check} disabled={busy}>
                  {busy ? <Spinner /> : <RefreshCw />}
                </Button>
              </Tooltip>
            )}
            <Button size="sm" variant="ghost" onClick={() => setConfirm(true)}>
              <Unlink /> Disconnect
            </Button>
          </>
        ) : (
          <Button size="sm" variant="outline" onClick={connect} disabled={!p.configured || p.isGuest}>
            <Link2 /> Connect
          </Button>
        )}
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={`Disconnect ${p.displayName}?`}
        description={
          p.autoLinked
            ? `Disconnect ${p.displayName} for this session? Cloud backups stored on your drive will remain intact.`
            : "MCPanel removes its stored sign-in and revokes access where the provider allows it. Files already in your cloud storage stay there."
        }
        confirmLabel="Disconnect"
        destructive
        onConfirm={disconnect}
      />
    </div>
  );
}

function formatEta(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  if (m < 60) return `${m}m ${s}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

function ActiveOperationsSection() {
  const { data: operations } = useCloudOperations();
  const qc = useQueryClient();
  const [cancellingIds, setCancellingIds] = useState<Set<string>>(new Set());

  const visibleOps = (operations ?? []).filter((op) => {
    return (
      op.state === "starting" ||
      op.state === "uploading" ||
      op.state === "downloading" ||
      op.state === "processing" ||
      op.state === "cancelling"
    );
  });

  if (visibleOps.length === 0) return null;

  const cancelOp = async (opId: string) => {
    setCancellingIds((prev) => new Set(prev).add(opId));
    try {
      await api.cloud.cancelOperation(opId);
      toast.info("Cancellation requested");
      void qc.invalidateQueries({ queryKey: qk.cloudOperations });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <div className="border-t border-border bg-surface-1/40 px-4 py-3">
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted">
        Active Cloud Operations
      </h4>
      <div className="space-y-2.5">
        {visibleOps.map((op) => {
          const isCancelling = op.state === "cancelling" || cancellingIds.has(op.id);
          const isCancelled = op.state === "cancelled";
          const isFailed = op.state === "failed";
          const isCompleted = op.state === "completed";
          const isFinished = isCancelled || isFailed || isCompleted;
          const pct = Math.round(
            op.percentage ??
              (op.totalBytes && op.totalBytes > 0 ? (op.bytesCompleted / op.totalBytes) * 100 : 0)
          );
          const providerLabel =
            op.provider === "google_drive" ? "Google Drive" : op.provider === "dropbox" ? "Dropbox" : op.provider;
          const isDownload = op.opType === "download" || op.opType === "restore";
          const actionText = isDownload ? "Downloading" : "Uploading";

          return (
            <div
              key={op.id}
              className="rounded-lg border border-border bg-surface-1 p-3 shadow-xs space-y-2"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 min-w-0">
                  {isDownload ? (
                    <CloudDownload className="size-4 shrink-0 text-primary" />
                  ) : (
                    <CloudUpload className="size-4 shrink-0 text-primary" />
                  )}
                  <span className="truncate text-xs font-medium text-fg">
                    {actionText} <span className="font-mono text-muted">{op.backupName}</span> → {providerLabel}
                  </span>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  {isCancelling ? (
                    <Badge tone="warning">Cancelling…</Badge>
                  ) : isCancelled ? (
                    <Badge tone="neutral">Cancelled</Badge>
                  ) : isFailed ? (
                    <Badge tone="danger">Failed</Badge>
                  ) : isCompleted ? (
                    <Badge tone="success">Completed</Badge>
                  ) : (
                    <Badge tone="info">{pct}%</Badge>
                  )}

                  {!isFinished && !isCancelling && (
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-6 px-2 text-xs text-muted hover:text-danger"
                      onClick={() => void cancelOp(op.id)}
                    >
                      Cancel
                    </Button>
                  )}
                </div>
              </div>

              {/* Progress bar */}
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-surface-2">
                <div
                  className={`h-full transition-all duration-300 ${
                    isFailed
                      ? "bg-danger"
                      : isCancelled
                        ? "bg-muted"
                        : isCompleted
                          ? "bg-success"
                          : "bg-primary"
                  }`}
                  style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
                />
              </div>

              {/* Status details line */}
              <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted">
                <div>
                  <span>{formatBytes(op.bytesCompleted)}</span>
                  {op.totalBytes != null && <span> / {formatBytes(op.totalBytes)}</span>}
                  {op.bytesPerSec != null && op.bytesPerSec > 0 && !isFinished && (
                    <span className="ml-2 font-mono">({formatBytes(op.bytesPerSec)}/s)</span>
                  )}
                </div>
                <div>
                  {isFailed && op.errorMessage && (
                    <span className="text-danger truncate max-w-xs">{op.errorMessage}</span>
                  )}
                  {isCancelled && <span>Cancelled</span>}
                  {isCompleted && <span className="text-success">Finished</span>}
                  {!isFinished && op.etaSeconds != null && op.etaSeconds > 0 && (
                    <span>ETA ~{formatEta(op.etaSeconds)}</span>
                  )}
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function CloudFilesSection({ providers }: { providers: CloudStatusDto[] }) {
  const qc = useQueryClient();
  const connectedProviders = providers.filter((p) => p.connected);
  const { data: files, isLoading, isFetching } = useCloudFiles(undefined, connectedProviders.length > 0);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [fileToDelete, setFileToDelete] = useState<CloudFileDto | null>(null);

  if (connectedProviders.length === 0) {
    return null;
  }

  const handleDownload = async (file: CloudFileDto) => {
    setDownloadingId(file.id);
    const toastId = toast.loading(`Starting download of "${file.name}"…`);
    try {
      await api.cloud.download(file.id, file.provider, file.name);
      toast.success(`Download started for "${file.name}"`, { id: toastId });
      void qc.invalidateQueries({ queryKey: qk.cloudOperations });
    } catch (e) {
      toast.error(errorMessage(e), { id: toastId });
    } finally {
      setDownloadingId(null);
    }
  };

  const handleDelete = async () => {
    if (!fileToDelete) return;
    const file = fileToDelete;
    try {
      await api.cloud.deleteFile(file.id, file.provider);
      toast.success(`Deleted ${file.name} from cloud`);
      void qc.invalidateQueries({ queryKey: qk.cloudFiles() });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setFileToDelete(null);
    }
  };

  const refreshFiles = () => void qc.invalidateQueries({ queryKey: qk.cloudFiles() });

  return (
    <div className="border-t border-border">
      <div className="bg-surface-1/50 flex items-center justify-between px-4 py-3">
        <div>
          <h4 className="flex items-center gap-2 text-[13px] font-medium text-fg">
            <CloudDownload className="text-primary size-4" /> Cloud Backups
          </h4>
          <p className="text-xs text-muted">
            Backups stored in your linked cloud storage. Download any cloud backup to restore or verify it locally.
          </p>
        </div>
        <Button size="icon-sm" variant="ghost" aria-label="Refresh cloud backups" onClick={refreshFiles} disabled={isFetching}>
          {isFetching ? <Spinner /> : <RefreshCw />}
        </Button>
      </div>

      {isLoading ? (
        <div className="flex justify-center p-6">
          <Spinner />
        </div>
      ) : !files || files.length === 0 ? (
        <div className="px-4 py-6 text-center text-xs text-muted">
          No backups uploaded to cloud storage yet. Use &ldquo;Upload to Cloud&rdquo; on any local backup to save a copy.
        </div>
      ) : (
        <div className="table-scroll">
          <table className="w-full text-[13px]">
            <thead className="border-b border-border text-left text-xs text-muted">
              <tr>
                <th className="px-4 py-2 font-medium">Backup File</th>
                <th className="px-4 py-2 font-medium">Provider</th>
                <th className="px-4 py-2 font-medium">Size</th>
                <th className="px-4 py-2 font-medium">Date</th>
                <th className="w-24 px-4 py-2 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {files.map((f) => (
                <tr key={f.id} className="border-b border-border last:border-0 hover:bg-surface-2">
                  <td className="px-4 py-2 font-mono text-xs text-fg">
                    <Tooltip content={f.id}>
                      <span>{f.name}</span>
                    </Tooltip>
                  </td>
                  <td className="px-4 py-2">
                    <Badge tone="neutral">{f.provider === "google_drive" ? "Google Drive" : f.provider === "dropbox" ? "Dropbox" : f.provider}</Badge>
                  </td>
                  <td className="px-4 py-2 text-fg">{formatBytes(f.sizeBytes)}</td>
                  <td className="px-4 py-2 text-muted">
                    {f.modifiedAt ? (
                      <Tooltip content={formatDateTime(f.modifiedAt)}>
                        <span>{formatRelative(f.modifiedAt)}</span>
                      </Tooltip>
                    ) : (
                      "—"
                    )}
                  </td>
                  <td className="px-4 py-2 text-right">
                    <div className="flex items-center justify-end gap-1">
                      <Tooltip content="Download to local backups">
                        <Button
                          size="icon-sm"
                          variant="ghost"
                          disabled={downloadingId === f.id}
                          onClick={() => void handleDownload(f)}
                          aria-label={`Download ${f.name}`}
                        >
                          {downloadingId === f.id ? <Spinner /> : <Download className="size-3.5" />}
                        </Button>
                      </Tooltip>
                      <Tooltip content="Delete from cloud">
                        <Button
                          size="icon-sm"
                          variant="ghost"
                          className="hover:text-danger"
                          onClick={() => setFileToDelete(f)}
                          aria-label={`Delete ${f.name} from cloud`}
                        >
                          <Trash2 className="size-3.5" />
                        </Button>
                      </Tooltip>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {fileToDelete && (
        <ConfirmDialog
          open={!!fileToDelete}
          onOpenChange={(open) => !open && setFileToDelete(null)}
          title={`Delete ${fileToDelete.name} from cloud storage?`}
          description="This file will be permanently removed from your cloud drive. Any local copies will remain untouched."
          confirmLabel="Delete from cloud"
          destructive
          onConfirm={handleDelete}
        />
      )}
    </div>
  );
}

/** Backups card: connect or disconnect cloud storage accounts, and manage cloud backups. */
export function CloudCard() {
  const { data } = useCloud();
  if (!data) return null;
  return (
    <Card>
      <CardHeader
        title="Cloud storage & Backups"
        description="Link Google Drive or Dropbox to store copies of your server backups offsite. Tokens are securely encrypted with the Windows Credential Manager."
      />
      <div className="divide-y divide-border">
        {data.map((p) => (
          <ProviderRow key={p.id} p={p} />
        ))}
      </div>
      <ActiveOperationsSection />
      <CloudFilesSection providers={data} />
      <div className="flex items-center gap-2 border-t border-border px-4 py-2.5 text-[11px] text-faint">
        <HardDrive className="size-3.5 text-muted" /> Cloud backups are encrypted at rest with age if Backup Encryption is enabled.
      </div>
    </Card>
  );
}
