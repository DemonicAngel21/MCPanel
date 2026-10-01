import { Link } from "@tanstack/react-router";
import { Copy, FileArchive } from "lucide-react";
import { toast } from "sonner";
import { PageBody } from "@/app/app-shell";
import { ActivityList } from "@/components/activity-list";
import { CrashHistory } from "@/components/crash-history";
import { InternetAccessSummary } from "@/components/playit-card";
import { SoftwareMark } from "@/components/software-mark";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatSoftwareBuild } from "@/lib/format";
import { useAudit, useJava, useServer } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

async function exportDiagnostics(serverId: string, name: string) {
  try {
    const safe = name.replace(/[^\w.-]+/g, "-").slice(0, 40) || "server";
    const grant = await api.dialog.saveFile(`${safe}-diagnostics.zip`);
    if (!grant) return;
    const n = await api.diagnostics.export(serverId, grant.token);
    toast.success(`Diagnostics saved (${n} files)`);
  } catch (e) {
    toast.error(errorMessage(e));
  }
}

export function ServerOverview() {
  const id = useServerId();
  const { data: server } = useServer(id);
  const { data: java } = useJava();
  const { data: audit } = useAudit(id, 8);
  if (!server) return null;
  const runtime = java?.find((j) => j.id === server.launch.javaRuntimeId);
  const address = `localhost:${server.port ?? 25565}`;

  return (
    <PageBody className="space-y-5">
      <div className="grid grid-cols-1 gap-5 xl:grid-cols-[1fr_380px]">
        <Card>
          <CardHeader title="Connection" description="Players on this computer or your local network can join with this address." />
          <div className="space-y-3 p-4">
            <div className="flex items-center gap-2">
              <code className="selectable rounded-md border border-border bg-surface-2 px-2.5 py-1.5 font-mono text-[13px] text-fg">{address}</code>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Copy address"
                onClick={() => navigator.clipboard.writeText(address).then(() => toast.success("Address copied"))}
              >
                <Copy />
              </Button>
            </div>
            <p className="text-xs text-muted">
              Other devices on your network use this computer's LAN IP address with port {server.port ?? 25565}. Windows may ask to allow Java through
              the firewall the first time the server starts.
            </p>
            <InternetAccessSummary serverId={server.id} port={server.port ?? 25565} />
          </div>
        </Card>
        <Card>
          <CardHeader
            title="Details"
            actions={
              <Tooltip content="Save a ZIP with logs, crash reports and settings for getting help. Worlds and secret files are never included; logs can contain player names and IP addresses.">
                <Button size="sm" variant="ghost" onClick={() => void exportDiagnostics(server.id, server.name)}>
                  <FileArchive /> Diagnostics
                </Button>
              </Tooltip>
            }
          />
          <dl className="grid grid-cols-[110px_1fr] gap-x-3 gap-y-2 p-4 text-xs">
            <dt className="text-muted">Software</dt>
            <dd className="flex items-center gap-2 text-fg">
              <SoftwareMark
                softwareId={server.software.softwareId}
                name={server.software.softwareName}
                size="sm"
                className="size-6 rounded-md [&_svg]:size-3.5"
              />
              {server.software.softwareName} {server.software.gameVersion}
              {server.software.build && ` · ${formatSoftwareBuild(server.software.softwareId, server.software.build)}`}
            </dd>
            <dt className="text-muted">Java</dt>
            <dd className="text-fg">
              {runtime ? `Java ${runtime.major} (${runtime.vendor ?? "unknown vendor"})` : <span className="text-warning">Not selected</span>}
            </dd>
            <dt className="text-muted">Memory</dt>
            <dd className="text-fg">
              {server.launch.minMemoryMb} – {server.launch.maxMemoryMb} MB
            </dd>
            <dt className="text-muted">Jar</dt>
            <dd className="selectable truncate font-mono text-fg">{server.software.jar}</dd>
            <dt className="text-muted">Folder</dt>
            <dd className="selectable font-mono break-all text-fg">{server.directory}</dd>
            {server.pid && (
              <>
                <dt className="text-muted">Process ID</dt>
                <dd className="text-fg tabular-nums">{server.pid}</dd>
              </>
            )}
          </dl>
        </Card>
      </div>

      <CrashHistory serverId={id} />

      <Card>
        <CardHeader
          title="Recent activity"
          actions={
            <Button asChild variant="ghost" size="sm">
              <Link to="/servers/$serverId/activity" params={{ serverId: id }}>
                View all
              </Link>
            </Button>
          }
        />
        <ActivityList entries={audit} />
      </Card>
    </PageBody>
  );
}
