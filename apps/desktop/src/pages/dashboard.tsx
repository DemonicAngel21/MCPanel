import { Link } from "@tanstack/react-router";
import { AlertTriangle, Coffee, Cpu, HardDrive, MemoryStick, Plus, Server } from "lucide-react";
import { useMemo } from "react";
import { PageBody, PageHeader } from "@/app/app-shell";
import { ActivityList } from "@/components/activity-list";
import { ServerControls } from "@/components/server-controls";
import { Sparkline } from "@/components/sparkline";
import { Button } from "@/components/ui/button";
import { Badge, Banner, Card, CardHeader, EmptyState, StatusDot } from "@/components/ui/primitives";
import { formatBytes, formatPercent } from "@/lib/format";
import { useAudit, useJava, useServers, useSystemMetrics } from "@/lib/queries";
import { stateMeta } from "@/lib/server-state";

function Stat({
  icon,
  label,
  value,
  sub,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  sub?: string;
  children?: React.ReactNode;
}) {
  return (
    <Card className="flex flex-col gap-2 p-4">
      <div className="flex items-center gap-2 text-xs text-muted [&_svg]:size-4">
        {icon}
        {label}
      </div>
      <div className="flex items-baseline gap-2">
        <span className="text-xl font-semibold text-fg tabular-nums">{value}</span>
        {sub && <span className="text-xs text-faint">{sub}</span>}
      </div>
      {children}
    </Card>
  );
}

export function DashboardPage() {
  const { data: servers } = useServers();
  const { data: metrics } = useSystemMetrics();
  const { data: java } = useJava();
  const { data: audit } = useAudit(null, 12);
  const cur = metrics?.current;
  const cpuPoints = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.cpuPercent] as [number, number]), [metrics]);
  const memPoints = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.memoryBytes] as [number, number]), [metrics]);
  const names = useMemo(() => Object.fromEntries((servers ?? []).map((s) => [s.id, s.name])), [servers]);
  const running = servers?.filter((s) => ["running", "starting", "detached"].includes(s.state)).length ?? 0;
  const validJava = java?.filter((j) => j.valid).length ?? 0;
  const lowDisks = cur?.disks.filter((d) => d.totalBytes > 0 && d.availableBytes / d.totalBytes < 0.05) ?? [];

  return (
    <>
      <PageHeader
        title="Dashboard"
        description={cur ? `${cur.osName} ${cur.osVersion}${cur.hostName ? ` · ${cur.hostName}` : ""}` : "Overview of this computer and your servers"}
        actions={
          <Button asChild variant="primary">
            <Link to="/servers/new">
              <Plus /> New server
            </Link>
          </Button>
        }
      />
      <PageBody className="space-y-5">
        {java && validJava === 0 && (
          <Banner
            tone="warning"
            icon={<Coffee />}
            title="No Java runtime found"
            actions={
              <Button asChild size="sm" variant="outline">
                <Link to="/java">Manage Java</Link>
              </Button>
            }
          >
            Minecraft servers need Java. Install a Java runtime (for example Eclipse Temurin) or add one manually.
          </Banner>
        )}
        {lowDisks.map((d) => (
          <Banner key={d.mountPoint} tone="danger" icon={<AlertTriangle />} title={`Drive ${d.mountPoint} is almost full`}>
            Only {formatBytes(d.availableBytes)} of {formatBytes(d.totalBytes)} free. Servers may fail to save their worlds.
          </Banner>
        ))}

        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          <Stat icon={<Server />} label="Servers" value={`${running} / ${servers?.length ?? 0}`} sub="running" />
          <Stat icon={<Cpu />} label="CPU (system)" value={formatPercent(cur?.cpuPercent)} sub={cur ? `${cur.cpuCount} threads` : undefined}>
            <Sparkline points={cpuPoints} max={100} format={(v) => `${v.toFixed(0)}%`} />
          </Stat>
          <Stat
            icon={<MemoryStick />}
            label="Memory (system)"
            value={formatBytes(cur?.memoryUsedBytes)}
            sub={cur ? `of ${formatBytes(cur.memoryTotalBytes)}` : undefined}
          >
            <Sparkline points={memPoints} max={cur?.memoryTotalBytes} color="var(--info)" format={(v) => formatBytes(v)} />
          </Stat>
        </div>

        <div className="grid grid-cols-1 gap-5 xl:grid-cols-[1fr_380px]">
          <Card>
            <CardHeader title="Servers" description="Status of every server managed by MCPanel" />
            {servers?.length === 0 ? (
              <EmptyState
                icon={<Server />}
                title="No servers yet"
                description="Create a new server in a minute, or import a server folder you already have."
                action={
                  <div className="flex gap-2">
                    <Button asChild variant="primary">
                      <Link to="/servers/new">Create server</Link>
                    </Button>
                    <Button asChild variant="outline">
                      <Link to="/servers/import">Import</Link>
                    </Button>
                  </div>
                }
              />
            ) : (
              <ul className="divide-y divide-border">
                {servers?.map((s) => {
                  const m = stateMeta(s.state);
                  return (
                    <li key={s.id} className="flex items-center gap-3 px-4 py-3">
                      <StatusDot tone={m.tone} pulse={m.pulse} />
                      <Link to="/servers/$serverId" params={{ serverId: s.id }} className="min-w-0 flex-1 hover:underline">
                        <p className="truncate text-[13px] font-medium text-fg">{s.name}</p>
                        <p className="truncate text-xs text-muted">
                          {s.software.softwareName} {s.software.gameVersion}
                          {s.port ? ` · port ${s.port}` : ""}
                          {s.onlinePlayers.length > 0 ? ` · ${s.onlinePlayers.length} online` : ""}
                        </p>
                      </Link>
                      <Badge tone={m.tone}>{m.label}</Badge>
                      <ServerControls server={s} compact />
                    </li>
                  );
                })}
              </ul>
            )}
          </Card>
          <div className="space-y-5">
            <Card>
              <CardHeader title="Disks" description="Source: operating system" />
              <ul className="space-y-3 p-4">
                {cur?.disks.map((d) => {
                  const used = d.totalBytes - d.availableBytes;
                  const pct = d.totalBytes > 0 ? used / d.totalBytes : 0;
                  return (
                    <li key={d.mountPoint}>
                      <div className="mb-1 flex items-center justify-between text-xs">
                        <span className="flex items-center gap-1.5 text-fg">
                          <HardDrive className="size-3.5 text-muted" />
                          {d.mountPoint} {d.name && <span className="text-faint">{d.name}</span>}
                        </span>
                        <span className="text-muted tabular-nums">{formatBytes(d.availableBytes)} free</span>
                      </div>
                      <div className="h-1.5 overflow-hidden rounded-full bg-surface-3">
                        <div
                          className={pct > 0.95 ? "h-full bg-danger" : pct > 0.85 ? "h-full bg-warning" : "h-full bg-accent"}
                          style={{ width: `${pct * 100}%` }}
                        />
                      </div>
                    </li>
                  );
                })}
                {!cur && <li className="text-xs text-faint">Collecting…</li>}
              </ul>
            </Card>
            <Card>
              <CardHeader
                title="Recent activity"
                actions={
                  <Button asChild variant="ghost" size="sm">
                    <Link to="/activity">View all</Link>
                  </Button>
                }
              />
              <ActivityList entries={audit} serverNames={names} />
            </Card>
          </div>
        </div>
      </PageBody>
    </>
  );
}
