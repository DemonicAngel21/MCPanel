import { Link } from "@tanstack/react-router";
import { Copy, Cpu, MemoryStick, Timer, Users } from "lucide-react";
import { useMemo } from "react";
import { toast } from "sonner";
import { PageBody } from "@/app/app-shell";
import { ActivityList } from "@/components/activity-list";
import { CrashHistory } from "@/components/crash-history";
import { Sparkline } from "@/components/sparkline";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, Tooltip } from "@/components/ui/primitives";
import { formatBytes, formatDuration, formatPercent } from "@/lib/format";
import { useAudit, useJava, useServer, useServerMetrics } from "@/lib/queries";
import { hasProcess } from "@/lib/server-state";
import { useServerId } from "./use-server-id";

function Metric({
  icon,
  label,
  value,
  source,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  source: string;
  children?: React.ReactNode;
}) {
  return (
    <Card className="flex flex-col gap-2 p-4">
      <div className="flex items-center justify-between text-xs text-muted">
        <span className="flex items-center gap-2 [&_svg]:size-4">
          {icon}
          {label}
        </span>
        <Tooltip content={`Source: ${source}`}>
          <span className="rounded bg-surface-3 px-1.5 py-px text-[10px] text-faint uppercase">{source}</span>
        </Tooltip>
      </div>
      <span className="text-xl font-semibold text-fg tabular-nums">{value}</span>
      {children}
    </Card>
  );
}

export function ServerOverview() {
  const id = useServerId();
  const { data: server } = useServer(id);
  const alive = !!server && hasProcess(server.state);
  const { data: metrics } = useServerMetrics(id, alive);
  const { data: java } = useJava();
  const { data: audit } = useAudit(id, 8);
  const cpu = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.cpuPercent] as [number, number]), [metrics]);
  const mem = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.memoryBytes] as [number, number]), [metrics]);
  if (!server) return null;
  const runtime = java?.find((j) => j.id === server.launch.javaRuntimeId);
  const address = `localhost:${server.port ?? 25565}`;
  const cur = alive ? metrics?.current : null;

  return (
    <PageBody className="space-y-5">
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
        <Metric icon={<Cpu />} label="CPU" source="OS process" value={cur ? formatPercent(cur.cpuPercent) : "—"}>
          <Sparkline points={cpu} max={100} format={(v) => `${v.toFixed(1)}%`} />
        </Metric>
        <Metric icon={<MemoryStick />} label="Memory" source="OS process" value={cur ? formatBytes(cur.memoryBytes) : "—"}>
          <Sparkline points={mem} max={server.launch.maxMemoryMb * 1024 * 1024} color="var(--info)" format={(v) => formatBytes(v)} />
        </Metric>
        <Metric icon={<Users />} label="Players online" source="console log" value={alive ? String(server.onlinePlayers.length) : "—"}>
          <p className="truncate text-xs text-muted">{server.onlinePlayers.join(", ") || (alive ? "Nobody online" : "Server not running")}</p>
        </Metric>
        <Metric icon={<Timer />} label="Uptime" source="MCPanel" value={alive ? formatDuration(metrics?.uptimeMs) : "—"}>
          <p className="text-xs text-muted">
            {server.state === "running" && server.readyAt && server.startedAt
              ? `Started in ${formatDuration(server.readyAt - server.startedAt)}`
              : " "}
          </p>
        </Metric>
      </div>
      <p className="-mt-2 text-[11px] text-faint">
        TPS and MSPT are not shown yet: they need a Minecraft-level metrics source, which arrives in a later version. MCPanel never estimates them.
      </p>

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
              the firewall the first time the server starts. Internet access through tunnels (Playit.gg) comes in a later version.
            </p>
          </div>
        </Card>
        <Card>
          <CardHeader title="Details" />
          <dl className="grid grid-cols-[110px_1fr] gap-x-3 gap-y-2 p-4 text-xs">
            <dt className="text-muted">Software</dt>
            <dd className="text-fg">
              {server.software.softwareName} {server.software.gameVersion}
              {server.software.build && ` #${server.software.build}`}
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
