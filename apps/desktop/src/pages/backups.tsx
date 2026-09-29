import { useQueryClient } from "@tanstack/react-query";
import { FolderOpen, FolderPen, RotateCcw } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import { BackupList } from "@/components/backup-list";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Banner, Card, CardHeader } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { LOCATION_WARNING_TEXT } from "@/lib/location";
import { qk, useBackupLocation, useBackups, useServers } from "@/lib/queries";
import { hasProcess } from "@/lib/server-state";
import { errorMessage } from "@/lib/utils";

function LocationCard() {
  const qc = useQueryClient();
  const { data: loc } = useBackupLocation();
  const [busy, setBusy] = useState(false);
  const change = async (pick: boolean) => {
    setBusy(true);
    try {
      let grant: string | null = null;
      if (pick) {
        const g = await api.dialog.pickFolder("Choose the backups folder");
        if (!g) return;
        grant = g.token;
      }
      qc.setQueryData(qk.backupLocation, await api.backups.setLocation(grant));
      toast.success("New backups will be saved in the new folder. Existing backups stay where they are.");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  if (!loc) return null;
  return (
    <Card>
      <CardHeader
        title="Backups folder"
        description={loc.availableBytes != null ? `${formatBytes(loc.availableBytes)} free on this drive` : undefined}
        actions={
          <>
            <Button size="sm" variant="ghost" onClick={() => api.backups.openFolder().catch((e) => toast.error(errorMessage(e)))}>
              <FolderOpen /> Open
            </Button>
            {!loc.isDefault && (
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => change(false)}>
                <RotateCcw /> Use default
              </Button>
            )}
            <Button size="sm" disabled={busy} onClick={() => change(true)}>
              <FolderPen /> Change…
            </Button>
          </>
        }
      />
      <div className="space-y-2 p-4">
        <p className="selectable font-mono text-xs text-fg">{loc.directory}</p>
        {loc.warnings.map((w) => (
          <Banner key={w} tone="warning" title={LOCATION_WARNING_TEXT[w] ?? w} />
        ))}
        <p className="text-xs text-muted">
          Backups are plain ZIP files you can open in Explorer. Keep copies on another drive to protect against disk failure. Backups that contain the
          Floodgate key are marked <span className="text-warning">Sensitive</span> — do not share them.
        </p>
      </div>
    </Card>
  );
}

export function BackupsPage() {
  const { data: servers } = useServers();
  const [server, setServer] = useState("all");
  const { data: backups } = useBackups(server === "all" ? null : server);
  const running = useMemo(() => new Set((servers ?? []).filter((s) => hasProcess(s.state)).map((s) => s.id)), [servers]);
  const total = (backups ?? []).filter((b) => b.status === "ready").reduce((n, b) => n + b.sizeBytes, 0);
  return (
    <>
      <PageHeader
        title="Backups"
        description="Local backups of all servers. Create backups and schedules from a server's Backups tab."
        actions={
          <Select
            aria-label="Show backups of"
            className="w-56"
            value={server}
            onValueChange={setServer}
            options={[{ value: "all", label: "All servers" }, ...(servers ?? []).map((s) => ({ value: s.id, label: s.name }))]}
          />
        }
      />
      <PageBody className="max-w-5xl space-y-5">
        <LocationCard />
        <Card>
          <CardHeader title={`${backups?.length ?? 0} backups`} description={formatBytes(total)} />
          <BackupList backups={backups} showServer isRunning={(id) => !!id && running.has(id)} />
        </Card>
      </PageBody>
    </>
  );
}
