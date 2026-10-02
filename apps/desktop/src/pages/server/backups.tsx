import { useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Archive, RefreshCw, Save } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import type { BackupPolicyDto } from "@/bindings/BackupPolicyDto";
import type { ServerDto } from "@/bindings/ServerDto";
import { PageBody } from "@/app/app-shell";
import { BackupList } from "@/components/backup-list";
import { CloudCard } from "@/components/cloud-card";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, Select } from "@/components/ui/overlays";
import { Card, CardHeader, Checkbox, Field, Input, Spinner, Switch } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes, formatCount, formatDateTime } from "@/lib/format";
import { qk, useBackupLocation, useBackupPolicy, useBackups, useDiskUsage, useServer } from "@/lib/queries";
import { hasProcess } from "@/lib/server-state";
import { errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

const INTERVALS = [
  { value: "15", label: "Every 15 minutes" },
  { value: "30", label: "Every 30 minutes" },
  { value: "60", label: "Every hour" },
  { value: "120", label: "Every 2 hours" },
  { value: "180", label: "Every 3 hours" },
  { value: "360", label: "Every 6 hours" },
  { value: "720", label: "Every 12 hours" },
  { value: "1440", label: "Every day" },
  { value: "10080", label: "Every week" },
];

/** Why a backup cannot be taken right now, if so. */
function backupBlocker(server: ServerDto): string | null {
  switch (server.state) {
    case "starting":
      return "Wait until the server has started.";
    case "stopping":
    case "restarting":
      return "The server is stopping.";
    case "detached":
      return "The console of this server is not connected; stop it first.";
    default:
      return server.operations.some((o) => o !== "editing_config") ? "Another operation is in progress." : null;
  }
}

function BackupNowDialog({ server, open, onOpenChange }: { server: ServerDto; open: boolean; onOpenChange: (o: boolean) => void }) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const live = server.state === "running";
  const start = async () => {
    setBusy(true);
    try {
      await api.backups.create(server.id, note.trim() || null);
      setNote("");
      onOpenChange(false);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        title={`Back up ${server.name}`}
        description={
          live
            ? "The server keeps running. MCPanel pauses automatic saving, saves the world, archives the files and turns saving back on."
            : "The whole server folder is archived as a ZIP file."
        }
        footer={
          <>
            <DialogClose asChild>
              <Button variant="ghost">Cancel</Button>
            </DialogClose>
            <Button variant="primary" onClick={start} disabled={busy}>
              {busy ? <Spinner className="text-accent-fg" /> : <Archive />} Back up
            </Button>
          </>
        }
      >
        <Field label="Note (optional)" hint="For example “before updating plugins”.">
          <Input
            value={note}
            maxLength={200}
            onChange={(e) => setNote(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void start()}
            autoFocus
          />
        </Field>
      </DialogContent>
    </Dialog>
  );
}

function ScheduleForm({ policy }: { policy: BackupPolicyDto }) {
  const qc = useQueryClient();
  const [enabled, setEnabled] = useState(policy.enabled);
  const [interval, setIntervalMinutes] = useState(String(policy.intervalMinutes));
  const [skipIdle, setSkipIdle] = useState(policy.skipIfIdle);
  const [keep, setKeep] = useState({
    keepLast: policy.keepLast,
    keepDaily: policy.keepDaily,
    keepWeekly: policy.keepWeekly,
    keepMonthly: policy.keepMonthly,
  });
  const [capGb, setCapGb] = useState(String(policy.maxTotalGb || ""));
  const [saving, setSaving] = useState(false);
  const intervals = INTERVALS.some((i) => i.value === interval) ? INTERVALS : [...INTERVALS, { value: interval, label: `Every ${interval} minutes` }];
  const num = (v: string) => Math.min(1000, Number(v.replace(/\D/g, "")) || 0);

  const save = async () => {
    setSaving(true);
    try {
      const p = await api.backups.updatePolicy(policy.serverId, {
        enabled,
        intervalMinutes: Number(interval),
        skipIfIdle: skipIdle,
        ...keep,
        maxTotalGb: Number(capGb) || 0,
      });
      qc.setQueryData(qk.backupPolicy(policy.serverId), p);
      toast.success(p.enabled ? "Backup schedule saved" : "Scheduled backups are off");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card>
      <CardHeader
        title="Schedule"
        actions={
          <label className="flex items-center gap-2 text-xs text-muted">
            {enabled ? "On" : "Off"}
            <Switch checked={enabled} onCheckedChange={setEnabled} aria-label="Scheduled backups" />
          </label>
        }
      />
      <div className="grid grid-cols-2 gap-4 p-4">
        <Field label="Frequency" hint={policy.enabled && policy.nextRunAt ? `Next backup around ${formatDateTime(policy.nextRunAt)}.` : undefined}>
          <Select value={interval} onValueChange={setIntervalMinutes} options={intervals} />
        </Field>
        <Field label=" ">
          <label className="flex items-start gap-2 pt-1.5 text-xs text-fg">
            <Checkbox checked={skipIdle} onCheckedChange={(c) => setSkipIdle(c === true)} className="mt-0.5" />
            <span>Skip when the server has not run since the last backup</span>
          </label>
        </Field>
        <div className="col-span-2">
          <p className="mb-2 text-xs text-muted">
            Keep scheduled backups (the newest of each day, week and month are kept). Manual and pre-restore backups are never deleted automatically.
          </p>
          <div className="grid grid-cols-4 gap-3">
            {(
              [
                ["keepLast", "Newest"],
                ["keepDaily", "Daily"],
                ["keepWeekly", "Weekly"],
                ["keepMonthly", "Monthly"],
              ] as const
            ).map(([k, label]) => (
              <Field key={k} label={label}>
                <Input inputMode="numeric" value={keep[k]} onChange={(e) => setKeep({ ...keep, [k]: num(e.target.value) })} />
              </Field>
            ))}
          </div>
        </div>
        <Field
          label="Size limit for scheduled backups (GB)"
          hint="Optional. When they use more, the oldest scheduled backups are deleted; the newest is always kept."
        >
          <Input inputMode="numeric" placeholder="No limit" value={capGb} onChange={(e) => setCapGb(e.target.value.replace(/\D/g, "").slice(0, 5))} />
        </Field>
      </div>
      <div className="flex justify-end border-t border-border px-4 py-3">
        <Button variant="primary" onClick={save} disabled={saving || keep.keepLast < 1}>
          {saving ? <Spinner className="text-accent-fg" /> : <Save />} Save schedule
        </Button>
      </div>
    </Card>
  );
}

function DiskUsageCard({ serverId }: { serverId: string }) {
  const { data: u, refetch, isFetching } = useDiskUsage(serverId);
  if (!u) return null;
  const parts: [string, number, string][] = [
    ["Worlds", u.worldsBytes, "var(--accent)"],
    ["Plugins, mods & config", u.contentBytes, "var(--info)"],
    ["Logs & crash reports", u.logsBytes, "var(--warning)"],
    ["Other files", u.otherBytes, "var(--muted)"],
    ["Backups", u.backupsBytes, "var(--faint)"],
  ];
  const sum = parts.reduce((n, p) => n + p[1], 0) || 1;
  return (
    <Card>
      <CardHeader
        title="Disk usage"
        description={
          u.driveFreeBytes != null && u.driveTotalBytes != null
            ? `${formatBytes(u.totalBytes + u.backupsBytes)} used by this server · ${formatBytes(u.driveFreeBytes)} free of ${formatBytes(u.driveTotalBytes)} on its drive`
            : `${formatBytes(u.totalBytes + u.backupsBytes)} used by this server`
        }
        actions={
          <Button size="icon-sm" variant="ghost" aria-label="Measure again" onClick={() => void refetch()} disabled={isFetching}>
            {isFetching ? <Spinner /> : <RefreshCw />}
          </Button>
        }
      />
      <div className="space-y-3 p-4">
        <div className="flex h-2.5 overflow-hidden rounded-full bg-surface-3">
          {parts.map(([label, n, color]) => (
            <div key={label} style={{ width: `${(n / sum) * 100}%`, background: color }} title={`${label}: ${formatBytes(n)}`} />
          ))}
        </div>
        <div className="grid grid-cols-2 gap-x-6 gap-y-1 text-xs md:grid-cols-3">
          {parts.map(([label, n, color]) => (
            <span key={label} className="flex items-center gap-2 text-muted">
              <span className="size-2 rounded-full" style={{ background: color }} />
              {label}
              <span className="ml-auto text-fg tabular-nums">{formatBytes(n)}</span>
            </span>
          ))}
        </div>
        {u.truncated && <p className="text-[11px] text-faint">Very many files: the measurement stopped early and shows a lower bound.</p>}
      </div>
    </Card>
  );
}

export function ServerBackups() {
  const id = useServerId();
  const { data: server } = useServer(id);
  const { data: backups } = useBackups(id);
  const { data: policy } = useBackupPolicy(id);
  const { data: location } = useBackupLocation();
  const [open, setOpen] = useState(false);
  if (!server) return null;
  const blocker = backupBlocker(server);
  const total = (backups ?? []).filter((b) => b.status === "ready").reduce((n, b) => n + b.sizeBytes, 0);

  return (
    <PageBody className="space-y-5">
      <Card>
        <CardHeader
          title="Backups"
          description={
            location && (
              <>
                {formatCount(backups?.length ?? 0, "backup")} · {formatBytes(total)} ·{" "}
                <Link
                  to="/backups"
                  title={`Backups folder: ${location.directory} (change on the Backups page)`}
                  className="inline-block max-w-[200px] truncate align-bottom text-muted underline-offset-2 hover:text-fg hover:underline sm:max-w-xs md:max-w-md"
                >
                  {location.directory}
                </Link>
              </>
            )
          }
          actions={
            <Button variant="primary" size="sm" disabled={!!blocker} title={blocker ?? undefined} onClick={() => setOpen(true)}>
              <Archive /> Back up now
            </Button>
          }
        />
        <BackupList backups={backups} isRunning={() => hasProcess(server.state)} />
      </Card>
      {policy && <ScheduleForm key={JSON.stringify(policy)} policy={policy} />}
      <CloudCard />
      <DiskUsageCard serverId={id} />
      <BackupNowDialog server={server} open={open} onOpenChange={setOpen} />
    </PageBody>
  );
}
