import { Link } from "@tanstack/react-router";
import { AlertTriangle, Archive, Coffee, Cpu, HardDrive, MemoryStick, Plus, Server, Users } from "lucide-react";
import { useMemo } from "react";
import type { ServerDto } from "@/bindings/ServerDto";
import type { BackupDto } from "@/bindings/BackupDto";
import { PageBody, PageHeader } from "@/app/app-shell";
import { ActivityList } from "@/components/activity-list";
import { PlayerHead } from "@/components/player-head";
import { ServerControls } from "@/components/server-controls";
import { SoftwareMark } from "@/components/software-mark";
import { Sparkline } from "@/components/sparkline";
import { cn, errorMessage } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Badge, Banner, Card, CardHeader, EmptyState, SkeletonRows, StatusDot } from "@/components/ui/primitives";
import { formatBytes, formatDuration, formatPercent, formatRelative } from "@/lib/format";
import { useAudit, useBackups, useJava, usePlayers, useServerMetrics, useServers, useSystemMetrics } from "@/lib/queries";
import { hasProcess, stateMeta } from "@/lib/server-state";

function Stat({
  icon,
  label,
  value,
  sub,
  children,
  tint = "text-accent",
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  sub?: string;
  children?: React.ReactNode;
  tint?: string;
}) {
  return (
    <Card className="flex min-w-0 flex-col gap-2 p-3.5">
      <div className="flex items-center gap-2 text-xs text-muted">
        <span className={cn("flex size-6 items-center justify-center rounded-md bg-current/12 [&_svg]:size-3.5", tint)}>{icon}</span>
        {label}
      </div>
      <div className="flex items-baseline gap-2">
        <span className="text-xl font-semibold text-fg tabular-nums">{value}</span>
        {sub && <span className="truncate text-xs text-faint">{sub}</span>}
      </div>
      {children}
    </Card>
  );
}

function ServerCard({ server }: { server: ServerDto }) {
  const running = hasProcess(server.state);
  const { data: metrics } = useServerMetrics(server.id, running);
  const hasOnlinePlayers = running && server.onlinePlayers.length > 0;
  const { data: players } = usePlayers(server.id, hasOnlinePlayers);
  const meta = stateMeta(server.state);
  const cpu = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.cpuPercent] as [number, number]), [metrics]);
  const memory = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.memoryBytes] as [number, number]), [metrics]);
  const tps = metrics?.tick?.tps;
  const onlinePlayers = server.onlinePlayers;
  const playerRows = players?.known.filter((player) => player.online) ?? [];

  return (
    <Card className="server-card min-w-0 overflow-hidden p-3.5">
      <div className="flex min-w-0 items-start gap-3">
        <SoftwareMark softwareId={server.software.softwareId} name={server.software.softwareName} />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <Link
              to="/servers/$serverId"
              params={{ serverId: server.id }}
              className="min-w-0 truncate text-[13px] font-semibold text-fg hover:text-accent-text focus-visible:rounded-sm"
            >
              {server.name}
            </Link>
            <StatusDot tone={meta.tone} pulse={meta.pulse} />
            <Badge tone={meta.tone} className="ml-auto shrink-0">
              {meta.label}
            </Badge>
          </div>
          <p className="mt-1 truncate text-xs text-muted">
            {server.software.softwareName} {server.software.gameVersion}
            {server.port ? ` · ${server.port}` : ""}
          </p>
        </div>
        <ServerControls server={server} compact />
      </div>

      <div className="mt-3 grid grid-cols-2 gap-3 border-t border-border pt-2.5">
        <div className="min-w-0">
          <div className="flex items-baseline justify-between gap-2">
            <span className="flex items-center gap-1.5 text-[11px] text-muted">
              <Cpu className="size-3.5 text-tint-cpu" /> CPU
            </span>
            <span className="font-mono text-xs text-fg tabular-nums">
              {running && metrics?.current ? formatPercent(metrics.current.cpuPercent) : "—"}
            </span>
          </div>
          <Sparkline points={running ? cpu : []} max={100} height={28} color="var(--tint-cpu)" format={(v) => `${v.toFixed(0)}%`} />
        </div>
        <div className="min-w-0">
          <div className="flex items-baseline justify-between gap-2">
            <span className="flex items-center gap-1.5 text-[11px] text-muted">
              <MemoryStick className="size-3.5 text-tint-memory" /> RAM
            </span>
            <span className="font-mono text-xs text-fg tabular-nums">
              {running && metrics?.current ? formatBytes(metrics.current.memoryBytes) : "—"}
            </span>
          </div>
          <Sparkline
            points={running ? memory : []}
            max={server.launch.maxMemoryMb * 1024 * 1024}
            height={28}
            color="var(--tint-memory)"
            format={formatBytes}
          />
        </div>
      </div>

      <div className="mt-2 flex min-w-0 items-center gap-2 border-t border-border pt-2.5 text-[11px]">
        <span className="flex min-w-0 items-center gap-1.5 truncate text-muted" title={onlinePlayers.join(", ")}>
          {playerRows.length > 0 ? (
            <span className="flex shrink-0 -space-x-1.5">
              {playerRows.slice(0, 3).map((player) => (
                <PlayerHead key={player.name} name={player.name} uuid={player.uuid} size="sm" className="size-5 rounded-md" />
              ))}
            </span>
          ) : (
            <Users className="size-3.5 shrink-0 text-tint-players" />
          )}
          {onlinePlayers.length ? `${onlinePlayers.length} online · ${onlinePlayers.join(", ")}` : "No players"}
        </span>
        {running && metrics?.uptimeMs != null && <span className="ml-auto shrink-0 font-mono text-faint">{formatDuration(metrics.uptimeMs)}</span>}
        {running && tps != null && <span className="shrink-0 font-mono text-accent-text">{tps.toFixed(1)} TPS</span>}
      </div>
    </Card>
  );
}

function BackupActivity({ backups, isError, retry }: { backups: BackupDto[] | undefined; isError: boolean; retry: () => void }) {
  const latest = [...(backups ?? [])].sort((a, b) => b.createdAt - a.createdAt).slice(0, 3);
  return (
    <Card>
      <CardHeader
        title="Backups"
        actions={
          <Button asChild size="sm" variant="ghost">
            <Link to="/backups">View all</Link>
          </Button>
        }
      />
      {isError ? (
        <EmptyState
          tone="danger"
          icon={<AlertTriangle />}
          title="Backups unavailable"
          action={
            <Button size="sm" variant="outline" onClick={retry}>
              Retry
            </Button>
          }
        />
      ) : !backups ? (
        <p className="p-4 text-xs text-muted">Loading…</p>
      ) : latest.length === 0 ? (
        <EmptyState
          icon={<Archive />}
          title="No backups yet"
          description="Backups you create or schedule appear here."
          action={
            <Button asChild size="sm" variant="outline">
              <Link to="/backups">Open backups</Link>
            </Button>
          }
        />
      ) : (
        <ul className="divide-y divide-border">
          {latest.map((backup) => (
            <li key={backup.id} className="flex items-center gap-2.5 px-3.5 py-2.5">
              <span
                className={cn(
                  "flex size-7 shrink-0 items-center justify-center rounded-md",
                  backup.status === "ready" ? "bg-accent-soft text-accent-text" : "bg-warning-soft text-warning",
                )}
              >
                <Archive className="size-3.5" />
              </span>
              <div className="min-w-0 flex-1">
                <p className="truncate text-xs font-medium text-fg">{backup.serverName}</p>
                <p className="text-[11px] text-faint">
                  {formatRelative(backup.createdAt)} · {formatBytes(backup.sizeBytes)}
                </p>
              </div>
              <Badge tone={backup.status === "ready" ? "success" : backup.status === "failed" ? "danger" : "info"}>{backup.status}</Badge>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

export function DashboardPage() {
  const serversQuery = useServers();
  const { data: servers } = serversQuery;
  const metricsQuery = useSystemMetrics();
  const { data: metrics } = metricsQuery;
  const { data: java } = useJava();
  const { data: audit } = useAudit(null, 12);
  const backupsQuery = useBackups(null);
  const { data: backups } = backupsQuery;
  const cur = metrics?.current;
  const cpuPoints = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.cpuPercent] as [number, number]), [metrics]);
  const memPoints = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.memoryBytes] as [number, number]), [metrics]);
  const names = useMemo(() => Object.fromEntries((servers ?? []).map((s) => [s.id, s.name])), [servers]);
  const running = servers?.filter((s) => hasProcess(s.state)).length ?? 0;
  const onlineCount = servers?.reduce((total, server) => total + server.onlinePlayers.length, 0) ?? 0;
  const validJava = java?.filter((j) => j.valid).length ?? 0;
  const lowDisks = cur?.disks.filter((d) => d.totalBytes > 0 && d.availableBytes / d.totalBytes < 0.05) ?? [];

  return (
    <>
      <PageHeader
        title="Dashboard"
        description={cur ? `${cur.osName} ${cur.osVersion}${cur.hostName ? ` · ${cur.hostName}` : ""}` : undefined}
        actions={
          <Button asChild variant="primary">
            <Link to="/servers/new">
              <Plus /> New server
            </Link>
          </Button>
        }
      />
      <PageBody className="space-y-4">
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
            Install Java or add a runtime manually.
          </Banner>
        )}
        {lowDisks.map((d) => (
          <Banner key={d.mountPoint} tone="danger" icon={<AlertTriangle />} title={`Drive ${d.mountPoint} is almost full`}>
            {formatBytes(d.availableBytes)} free of {formatBytes(d.totalBytes)}. Servers may fail to save their worlds.
          </Banner>
        ))}

        <div className="grid grid-cols-2 gap-3 xl:grid-cols-4">
          <Stat icon={<Server />} label="Servers" value={`${running} / ${servers?.length ?? 0}`} sub="running">
            <div className="mt-auto flex h-[38px] flex-col justify-end gap-1.5">
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-surface-2">
                <div
                  className="h-full rounded-full bg-accent transition-all duration-300"
                  style={{ width: `${servers?.length ? Math.min(100, Math.round((running / servers.length) * 100)) : 0}%` }}
                />
              </div>
              <span className="text-[11px] text-faint">
                {servers?.length ? `${Math.round((running / servers.length) * 100)}% active` : "No servers configured"}
              </span>
            </div>
          </Stat>
          <Stat icon={<Users />} tint="text-tint-players" label="Players" value={`${onlineCount}`} sub="online">
            <div className="mt-auto flex h-[38px] flex-col justify-end gap-1.5">
              <div className="flex items-center gap-1.5 overflow-hidden text-[11px] text-faint">
                {onlineCount > 0 ? (
                  <span className="truncate text-tint-players">
                    Active across {servers?.filter((s) => s.onlinePlayers.length > 0).length ?? 0} server
                    {(servers?.filter((s) => s.onlinePlayers.length > 0).length ?? 0) === 1 ? "" : "s"}
                  </span>
                ) : (
                  <span>No active sessions</span>
                )}
              </div>
            </div>
          </Stat>
          <Stat
            icon={<Cpu />}
            tint="text-tint-cpu"
            label="System CPU"
            value={formatPercent(cur?.cpuPercent)}
            sub={cur ? `${cur.cpuCount} threads` : undefined}
          >
            <Sparkline points={cpuPoints} max={100} height={38} color="var(--tint-cpu)" format={(v) => `${v.toFixed(0)}%`} />
          </Stat>
          <Stat
            icon={<MemoryStick />}
            tint="text-tint-memory"
            label="System RAM"
            value={formatBytes(cur?.memoryUsedBytes)}
            sub={cur ? `of ${formatBytes(cur.memoryTotalBytes)}` : undefined}
          >
            <Sparkline points={memPoints} max={cur?.memoryTotalBytes} height={38} color="var(--tint-memory)" format={formatBytes} />
          </Stat>
        </div>

        <div className="grid grid-cols-1 items-start gap-4 2xl:grid-cols-[minmax(0,1fr)_340px]">
          <Card className="min-w-0">
            <CardHeader
              title="Servers"
              actions={
                <Button asChild variant="ghost" size="sm">
                  <Link to="/servers">View all</Link>
                </Button>
              }
            />
            {serversQuery.isLoading ? (
              <SkeletonRows rows={3} />
            ) : serversQuery.isError ? (
              <EmptyState
                tone="danger"
                icon={<AlertTriangle />}
                title="Servers unavailable"
                description={errorMessage(serversQuery.error)}
                action={
                  <Button size="sm" variant="outline" onClick={() => void serversQuery.refetch()}>
                    Retry
                  </Button>
                }
              />
            ) : servers?.length === 0 ? (
              <EmptyState
                icon={<Server />}
                title="No servers yet"
                description="Create or import a server to get started."
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
              <ul className="grid grid-cols-1 gap-3 p-3 md:grid-cols-2">
                {servers?.map((server) => (
                  <li key={server.id} className="min-w-0">
                    <ServerCard server={server} />
                  </li>
                ))}
              </ul>
            )}
          </Card>
          <div className="grid min-w-0 grid-cols-1 gap-4 lg:grid-cols-2 2xl:grid-cols-1">
            <Card>
              <CardHeader title="Storage" />
              <ul className="space-y-3 p-3.5">
                {cur?.disks.map((d) => {
                  const used = d.totalBytes - d.availableBytes;
                  const pct = d.totalBytes > 0 ? used / d.totalBytes : 0;
                  const tone = pct > 0.95 ? "bg-danger" : pct > 0.85 ? "bg-warning" : "bg-tint-disk";
                  return (
                    <li key={d.mountPoint}>
                      <div className="mb-1 flex items-center justify-between gap-2 text-xs">
                        <span className="flex min-w-0 items-center gap-1.5 truncate text-fg">
                          <HardDrive className="size-3.5 shrink-0 text-muted" />
                          {d.mountPoint}
                          {d.name && <span className="truncate text-faint">{d.name}</span>}
                        </span>
                        <span className="shrink-0 font-mono text-[11px] text-muted tabular-nums">{Math.round(pct * 100)}% used</span>
                      </div>
                      <div
                        className="metric-track h-1.5 overflow-hidden rounded-full"
                        title={`${formatBytes(used)} used of ${formatBytes(d.totalBytes)}`}
                      >
                        <div className={cn("metric-fill h-full rounded-full", tone)} style={{ width: `${Math.min(100, pct * 100)}%` }} />
                      </div>
                      <p className="mt-1 text-[11px] text-faint">{formatBytes(d.availableBytes)} free</p>
                    </li>
                  );
                })}
                {!cur && (
                  <li className="text-xs text-faint">{metricsQuery.isError ? "System metrics unavailable." : "Collecting system metrics…"}</li>
                )}
                {cur?.disks.length === 0 && <li className="text-xs text-faint">No disks reported.</li>}
              </ul>
            </Card>
            <BackupActivity backups={backups} isError={backupsQuery.isError} retry={() => void backupsQuery.refetch()} />
            <Card className="lg:col-span-2 2xl:col-span-1">
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
