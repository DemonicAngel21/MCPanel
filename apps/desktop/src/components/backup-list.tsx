import { AlertTriangle, Archive, ArchiveRestore, FolderSearch, KeyRound, Lock, MoreHorizontal, ShieldCheck, Trash2 } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import type { BackupDto } from "@/bindings/BackupDto";
import type { RestorePreviewDto } from "@/bindings/RestorePreviewDto";
import { SoftwareMark } from "@/components/software-mark";
import { Button } from "@/components/ui/button";
import {
  ConfirmDialog,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { Badge, EmptyState, Skeleton, Spinner, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes, formatDateTime, formatRelative } from "@/lib/format";
import { waitForJob } from "@/lib/jobs";
import { errorMessage } from "@/lib/utils";

const KIND_LABEL: Record<string, string> = {
  manual: "Manual",
  scheduled: "Scheduled",
  pre_restore: "Before restore",
};

async function verify(b: BackupDto) {
  try {
    const job = await waitForJob(await api.backups.verify(b.id));
    const report = job.result as { ok?: boolean; filesChecked?: number; problems?: string[] } | null;
    if (job.status !== "succeeded") return; // the job toast reports failures
    if (report?.ok) toast.success("Backup verified", { description: `${report.filesChecked ?? 0} files match their recorded hashes.` });
    else toast.error("This backup is damaged", { description: report?.problems?.slice(0, 3).join("\n"), duration: 15_000 });
  } catch (e) {
    toast.error(errorMessage(e));
  }
}

function RestoreDialog({ backup, running, onClose }: { backup: BackupDto; running: boolean; onClose: () => void }) {
  const q = useQuery({ queryKey: ["backups", "preview", backup.id], queryFn: () => api.backups.restorePreview(backup.id), gcTime: 0, retry: false });
  const preview: RestorePreviewDto | undefined = q.data;
  const error = q.error ? errorMessage(q.error) : null;
  return (
    <ConfirmDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={`Restore the backup from ${formatDateTime(backup.createdAt)}?`}
      description="The server's files are replaced with the files in this backup. MCPanel first backs up the current state, so you can undo this."
      confirmLabel="Restore"
      destructive
      confirmDisabled={running || !preview}
      onConfirm={async () => {
        try {
          await api.backups.restore(backup.id);
        } catch (e) {
          toast.error(errorMessage(e));
        }
      }}
    >
      <div className="space-y-3 text-xs">
        {running && (
          <p className="flex items-start gap-2 rounded-md bg-warning-soft p-2 text-warning">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" /> Stop the server before restoring.
          </p>
        )}
        {error && <p className="text-danger">{error}</p>}
        {!preview && !error && (
          <div role="status" aria-label="Loading" className="space-y-2">
            <Skeleton className="h-3 w-2/3" />
            <Skeleton className="h-3 w-1/2" />
            <Skeleton className="h-3 w-3/5" />
          </div>
        )}
        {preview && (
          <>
            <dl className="grid grid-cols-4 gap-2 text-center">
              {[
                ["Changed", preview.changed],
                ["Removed", preview.removed],
                ["Added back", preview.added],
                ["Unchanged", preview.unchanged],
              ].map(([label, n]) => (
                <div key={label} className="rounded-md border border-border p-2">
                  <dd className="text-sm font-semibold text-fg">{n}</dd>
                  <dt className="text-muted">{label}</dt>
                </div>
              ))}
            </dl>
            {preview.changedJars.length > 0 && (
              <div>
                <p className="mb-1 text-muted">Server, plugin and mod jars that differ:</p>
                <ul className="selectable max-h-28 overflow-y-auto rounded-md border border-border p-2 font-mono text-[11px]">
                  {preview.changedJars.map((j) => (
                    <li key={j}>{j}</li>
                  ))}
                </ul>
              </div>
            )}
            {preview.removedSample.length > 0 && (
              <div>
                <p className="mb-1 text-muted">
                  Files created after the backup that will be removed
                  {preview.removed > preview.removedSample.length && ` (first ${preview.removedSample.length})`}:
                </p>
                <ul className="selectable max-h-28 overflow-y-auto rounded-md border border-border p-2 font-mono text-[11px]">
                  {preview.removedSample.map((j) => (
                    <li key={j}>{j}</li>
                  ))}
                </ul>
              </div>
            )}
            <p className="text-muted">
              {formatBytes(preview.totalBytes)} will be restored. MCPanel's own data (trash) is kept.
              {backup.gameVersion && ` The server is recorded as ${backup.gameVersion} again.`}
            </p>
          </>
        )}
      </div>
    </ConfirmDialog>
  );
}

function StatusCell({ b }: { b: BackupDto }) {
  if (b.status === "creating")
    return (
      <span className="flex items-center gap-1.5 text-muted">
        <Spinner className="size-3" /> Creating…
      </span>
    );
  if (b.status === "failed")
    return (
      <Tooltip content={b.errorMessage ?? "Failed"}>
        <span className="text-danger">Failed</span>
      </Tooltip>
    );
  if (!b.filePresent) return <span className="text-danger">File missing</span>;
  return <span className="text-fg">{formatBytes(b.sizeBytes)}</span>;
}

/** Backups table with restore / verify / reveal / delete actions. */
export function BackupList({
  backups,
  isRunning,
  showServer,
}: {
  backups: BackupDto[] | undefined;
  /** Whether the backup's server currently has a process. */
  isRunning: (serverId: string | null) => boolean;
  showServer?: boolean;
}) {
  const [restore, setRestore] = useState<BackupDto | null>(null);
  const [remove, setRemove] = useState<BackupDto | null>(null);
  const [reveal, setReveal] = useState<BackupDto | null>(null);

  if (!backups) return <Spinner className="m-6" />;
  if (backups.length === 0) return <EmptyState icon={<Archive />} title="No backups yet" description="Backups you create or schedule appear here." />;

  const doReveal = (b: BackupDto) => api.backups.reveal(b.id).catch((e) => toast.error(errorMessage(e)));

  return (
    <>
      <div className="table-scroll">
      <table className="w-full text-[13px]">
        <thead className="border-b border-border text-left text-xs text-muted">
          <tr>
            <th className="px-4 py-2 font-medium">Created</th>
            {showServer && <th className="px-4 py-2 font-medium">Server</th>}
            <th className="px-4 py-2 font-medium">Type</th>
            <th className="px-4 py-2 font-medium">Size</th>
            <th className="px-4 py-2 font-medium">Note</th>
            <th className="w-10" />
          </tr>
        </thead>
        <tbody>
          {backups.map((b) => {
            const usable = b.status === "ready" && b.filePresent;
            return (
              <tr key={b.id} className="border-b border-border last:border-0 hover:bg-surface-2">
                <td className="px-4 py-2">
                  <Tooltip content={b.fileName}>
                    <span className="text-fg">{formatDateTime(b.createdAt)}</span>
                  </Tooltip>
                  <span className="ml-2 text-xs text-muted">{formatRelative(b.createdAt)}</span>
                </td>
                {showServer && <td className="px-4 py-2 text-fg"><span className="inline-flex items-center gap-2"><SoftwareMark softwareId={b.softwareId} size="sm" className="size-6 rounded-md [&_svg]:size-3.5" />{b.serverName}</span></td>}
                <td className="px-4 py-2">
                  <div className="flex flex-wrap items-center gap-1">
                    <Badge tone={b.kind === "pre_restore" ? "info" : "neutral"}>{KIND_LABEL[b.kind] ?? b.kind}</Badge>
                    {b.live && (
                      <Tooltip content="Taken while the server was running (saving paused)">
                        <Badge tone="success">Live</Badge>
                      </Tooltip>
                    )}
                    {b.encrypted && (
                      <Tooltip content="Encrypted with your backup key. Opening it elsewhere needs the Recovery Kit and its passphrase.">
                        <Badge tone="info">
                          <Lock className="size-3" /> Encrypted
                        </Badge>
                      </Tooltip>
                    )}
                    {b.containsSensitive && (
                      <Tooltip content="Contains highly sensitive files (e.g. the Floodgate key). Do not share this backup.">
                        <Badge tone="warning">
                          <KeyRound className="size-3" /> Sensitive
                        </Badge>
                      </Tooltip>
                    )}
                    {b.skipped.length > 0 && (
                      <Tooltip content={`Not included: ${b.skipped.map((s) => `${s.path} (${s.reason})`).join(", ")}`}>
                        <Badge tone="warning">{b.skipped.length} skipped</Badge>
                      </Tooltip>
                    )}
                  </div>
                </td>
                <td className="px-4 py-2">
                  <StatusCell b={b} />
                </td>
                <td className="max-w-56 truncate px-4 py-2 text-muted">{b.note}</td>
                <td className="px-2 py-1 text-right">
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon-sm" aria-label="Backup actions" disabled={b.status === "creating"}>
                        <MoreHorizontal />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent>
                      <DropdownMenuItem disabled={!usable || !b.serverId} onSelect={() => setRestore(b)}>
                        <ArchiveRestore /> Restore…
                      </DropdownMenuItem>
                      <DropdownMenuItem disabled={!usable} onSelect={() => void verify(b)}>
                        <ShieldCheck /> Verify
                      </DropdownMenuItem>
                      <DropdownMenuItem disabled={!usable} onSelect={() => (b.containsSensitive ? setReveal(b) : void doReveal(b))}>
                        <FolderSearch /> Show in Explorer
                      </DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem destructive onSelect={() => setRemove(b)}>
                        <Trash2 /> Delete…
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      </div>

      {restore && <RestoreDialog key={restore.id} backup={restore} running={isRunning(restore.serverId)} onClose={() => setRestore(null)} />}
      <ConfirmDialog
        open={!!remove}
        onOpenChange={(o) => !o && setRemove(null)}
        title="Delete this backup?"
        description={remove ? `${remove.fileName} is deleted permanently from disk.` : undefined}
        confirmLabel="Delete"
        destructive
        onConfirm={async () => {
          if (!remove) return;
          try {
            await api.backups.delete(remove.id);
            toast.success("Backup deleted");
          } catch (e) {
            toast.error(errorMessage(e));
          }
        }}
      />
      <ConfirmDialog
        open={!!reveal}
        onOpenChange={(o) => !o && setReveal(null)}
        title="This backup contains sensitive files"
        description="It includes highly sensitive files such as the Floodgate key, which lets anyone impersonate Bedrock players on your server. Do not upload or share this archive."
        confirmLabel="Show in Explorer"
        onConfirm={() => {
          if (reveal) void doReveal(reveal);
        }}
      />
    </>
  );
}
