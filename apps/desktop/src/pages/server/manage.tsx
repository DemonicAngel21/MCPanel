import { Link } from "@tanstack/react-router";
import { Archive, Cpu, FileText, FolderOpen, Gauge, Info, MemoryStick, SlidersHorizontal, SquareTerminal, Timer, Users } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { PageBody } from "@/app/app-shell";
import { ServerControls } from "@/components/server-controls";
import { TimeChart } from "@/components/time-chart";
import { Button } from "@/components/ui/button";
import { Badge, Card, CardHeader, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes, formatDuration, formatPercent } from "@/lib/format";
import { usePlayers, useServer, useServerMetrics } from "@/lib/queries";
import { hasProcess, stateMeta } from "@/lib/server-state";
import { cn, errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

const RANGES = [
  { label: "5 min", ms: 5 * 60_000 },
  { label: "15 min", ms: 15 * 60_000 },
  { label: "30 min", ms: 30 * 60_000 },
] as const;

function Tile({ icon, label, value, sub, source }: { icon: React.ReactNode; label: string; value: string; sub?: string; source: string }) {
  return (
    <Card className="flex flex-col gap-1 p-4">
      <div className="flex items-center justify-between gap-2 text-xs text-muted">
        <span className="flex items-center gap-2 [&_svg]:size-4">
          {icon}
          {label}
        </span>
        <Tooltip content={`Source: ${source}`}>
          <button type="button" aria-label={`${label} source: ${source}`} className="cursor-default text-faint hover:text-muted [&_svg]:size-3.5">
            <Info />
          </button>
        </Tooltip>
      </div>
      <span className="text-xl font-semibold text-fg tabular-nums">{value}</span>
      <span className="truncate text-xs text-muted">{sub ?? " "}</span>
    </Card>
  );
}

function ChartCard({ title, value, children }: { title: string; value: string; children: React.ReactNode }) {
  return (
    <div className="space-y-2 rounded-md border border-border p-3">
      <div className="flex items-baseline justify-between gap-2">
        <p className="text-xs font-medium text-muted">{title}</p>
        <p className="font-mono text-xs text-fg tabular-nums">{value}</p>
      </div>
      {children}
    </div>
  );
}

/** Basic server management: power, live resource use and performance graphs. */
export function ServerManage() {
  const id = useServerId();
  const { data: server } = useServer(id);
  const alive = !!server && hasProcess(server.state);
  const { data: metrics } = useServerMetrics(id, alive);
  const { data: players } = usePlayers(id);
  const [range, setRange] = useState<number>(RANGES[1].ms);

  const cpu = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.cpuPercent] as [number, number]), [metrics]);
  const mem = useMemo(() => (metrics?.history ?? []).map((p) => [p.at, p.memoryBytes / 1024 / 1024] as [number, number]), [metrics]);
  const tps = useMemo(() => (metrics?.tickHistory ?? []).flatMap((t) => (t.tps != null ? [[t.at, t.tps] as [number, number]] : [])), [metrics]);
  const mspt = useMemo(() => (metrics?.tickHistory ?? []).flatMap((t) => (t.mspt != null ? [[t.at, t.mspt] as [number, number]] : [])), [metrics]);

  if (!server) return null;
  const meta = stateMeta(server.state);
  const cur = alive ? metrics?.current : null;
  const tick = alive ? metrics?.tick : null;
  const tickSource = metrics?.tickSource ?? null;
  const tickLabel = tickSource === "paper_commands" ? "tps / mspt command" : "tick query";
  const maxMb = server.launch.maxMemoryMb;
  const online = server.onlinePlayers;
  const hasHistory = cpu.length > 1 || mem.length > 1;
  const openFolder = () => api.servers.openFolder(server.id).catch((e) => toast.error(errorMessage(e)));

  return (
    <PageBody className="space-y-5">
      <Card>
        <div className="flex flex-wrap items-center justify-between gap-4 p-4">
          <div className="space-y-1">
            <p className="flex items-center gap-2 text-sm font-semibold text-fg">
              Power <Badge tone={meta.tone}>{meta.label}</Badge>
            </p>
            <p className="text-xs text-muted">
              {alive && metrics?.uptimeMs != null
                ? `Up for ${formatDuration(metrics.uptimeMs)}${
                    server.state === "running" && server.readyAt && server.startedAt
                      ? ` · started in ${formatDuration(server.readyAt - server.startedAt)}`
                      : ""
                  }`
                : server.lastExitCode != null
                  ? `Last exit code ${server.lastExitCode}`
                  : "The server is not running."}
            </p>
          </div>
          <ServerControls server={server} />
        </div>
        <div className="flex flex-wrap gap-1.5 border-t border-border px-4 py-2.5">
          <Button asChild size="sm" variant="ghost">
            <Link to="/servers/$serverId/console" params={{ serverId: server.id }}>
              <SquareTerminal /> Console
            </Link>
          </Button>
          <Button asChild size="sm" variant="ghost">
            <Link to="/servers/$serverId/properties" params={{ serverId: server.id }}>
              <SlidersHorizontal /> Properties
            </Link>
          </Button>
          <Button asChild size="sm" variant="ghost">
            <Link to="/servers/$serverId/backups" params={{ serverId: server.id }}>
              <Archive /> Backups
            </Link>
          </Button>
          <Button asChild size="sm" variant="ghost">
            <Link to="/servers/$serverId/files" params={{ serverId: server.id }} search={{ path: "" }}>
              <FileText /> Files
            </Link>
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void openFolder()}>
            <FolderOpen /> Open folder
          </Button>
        </div>
      </Card>

      <div className="grid grid-cols-2 gap-4 lg:grid-cols-3 xl:grid-cols-5">
        <Tile icon={<Cpu />} label="CPU" source="OS process" value={cur ? formatPercent(cur.cpuPercent) : "—"} sub="of this computer" />
        <Tile
          icon={<MemoryStick />}
          label="Memory"
          source="OS process"
          value={cur ? formatBytes(cur.memoryBytes) : "—"}
          sub={`Java heap limit ${maxMb} MB`}
        />
        <Tile
          icon={<Users />}
          label="Players"
          source="console log"
          value={alive ? `${online.length}${players?.maxPlayers != null ? ` / ${players.maxPlayers}` : ""}` : "—"}
          sub={alive ? online.join(", ") || "Nobody online" : "Server not running"}
        />
        <Tile
          icon={<Gauge />}
          label={tick?.tpsCalculated ? "TPS (calculated)" : "TPS"}
          source={tickSource ? tickLabel : "not available"}
          value={tick?.tps != null ? tick.tps.toFixed(1) : "—"}
          sub="20 is full speed"
        />
        <Tile
          icon={<Timer />}
          label="MSPT"
          source={tickSource ? tickLabel : "not available"}
          value={tick?.mspt != null ? `${tick.mspt.toFixed(1)} ms` : "—"}
          sub="Limit 50 ms per tick"
        />
      </div>

      <Card>
        <CardHeader
          title="Performance"
          description={
            tickSource || !alive
              ? "Recorded while the server runs; MCPanel keeps the last 30 minutes."
              : "Recorded while the server runs. TPS and MSPT are not available for this server's software or version; MCPanel never estimates them."
          }
          actions={
            <div role="radiogroup" aria-label="Time range" className="flex rounded-md border border-border p-0.5">
              {RANGES.map((r) => (
                <button
                  key={r.ms}
                  role="radio"
                  aria-checked={range === r.ms}
                  onClick={() => setRange(r.ms)}
                  className={cn(
                    "cursor-default rounded px-2 py-0.5 text-xs transition-colors duration-150",
                    range === r.ms ? "bg-surface-3 font-medium text-fg" : "text-muted hover:text-fg",
                  )}
                >
                  {r.label}
                </button>
              ))}
            </div>
          }
        />
        {!hasHistory && !alive ? (
          <p className="p-6 text-center text-xs text-muted">Start the server to record its performance.</p>
        ) : (
          <div className="grid grid-cols-1 gap-4 p-4 lg:grid-cols-2">
            <ChartCard title="CPU" value={cur ? formatPercent(cur.cpuPercent) : "—"}>
              <TimeChart label="CPU usage over time" points={cpu} windowMs={range} max={100} format={(v) => `${v.toFixed(0)}%`} />
            </ChartCard>
            <ChartCard title="Memory" value={cur ? formatBytes(cur.memoryBytes) : "—"}>
              <TimeChart
                label="Memory usage over time"
                points={mem}
                windowMs={range}
                max={Math.max(maxMb * 1.1, ...mem.map((p) => p[1]))}
                threshold={maxMb}
                color="var(--info)"
                format={(v) => (v >= 1024 ? `${(v / 1024).toFixed(1)} GB` : `${v.toFixed(0)} MB`)}
              />
            </ChartCard>
            {tickSource && (
              <>
                <ChartCard title="TPS" value={tick?.tps != null ? tick.tps.toFixed(1) : "—"}>
                  <TimeChart label="Ticks per second over time" points={tps} windowMs={range} max={21} format={(v) => v.toFixed(0)} />
                </ChartCard>
                <ChartCard title="MSPT" value={tick?.mspt != null ? `${tick.mspt.toFixed(1)} ms` : "—"}>
                  <TimeChart
                    label="Milliseconds per tick over time"
                    points={mspt}
                    windowMs={range}
                    threshold={50}
                    color="var(--warning)"
                    format={(v) => `${v.toFixed(0)} ms`}
                  />
                </ChartCard>
              </>
            )}
          </div>
        )}
      </Card>
    </PageBody>
  );
}
