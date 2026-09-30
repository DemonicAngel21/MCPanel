import { ChevronRight } from "lucide-react";
import { useState } from "react";
import type { CrashEventDto } from "@/bindings/CrashEventDto";
import { Badge, Card, CardHeader } from "@/components/ui/primitives";
import { formatDateTime, formatRelative } from "@/lib/format";
import { useCrashes } from "@/lib/queries";
import { cn } from "@/lib/utils";

const ACTION: Record<string, { label: string; tone: "success" | "warning" | "danger" | "neutral" }> = {
  restart: { label: "Restarted automatically", tone: "success" },
  gave_up: { label: "Too many crashes — not restarted", tone: "danger" },
  not_restartable: { label: "Not restartable", tone: "warning" },
  disabled: { label: "Auto-restart off", tone: "neutral" },
};

const KIND: Record<string, string> = {
  out_of_memory: "Out of memory",
  watchdog: "Stopped responding",
  crash_report: "Crash report",
  port_in_use: "Port in use",
  eula: "EULA not accepted",
  java_too_old: "Java too old",
  jar_missing: "Server jar missing",
  memory: "Memory setting",
  world_incomplete: "World not fully created",
  unknown: "Unexpected exit",
};

function Row({ c }: { c: CrashEventDto }) {
  const [open, setOpen] = useState(false);
  const action = ACTION[c.action] ?? { label: c.action, tone: "neutral" as const };
  return (
    <li className="border-b border-border last:border-0">
      <button type="button" onClick={() => setOpen(!open)} className="flex w-full items-start gap-2 px-4 py-2.5 text-left hover:bg-surface-2">
        <ChevronRight className={cn("mt-0.5 size-4 shrink-0 text-faint transition-transform", open && "rotate-90")} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[13px] font-medium text-fg">{KIND[c.kind] ?? c.kind}</span>
            <Badge tone={action.tone}>{action.label}</Badge>
            {c.action === "restart" && c.attempt > 1 && <Badge>attempt {c.attempt}</Badge>}
          </div>
          <p className="truncate text-xs text-muted">{c.message}</p>
          {c.suspects.length > 0 && (
            <p className="mt-0.5 truncate text-xs text-warning">Likely involved: {c.suspects.map((s) => `${s.name} (${s.fileName})`).join(", ")}</p>
          )}
        </div>
        <span className="shrink-0 text-xs text-muted" title={formatDateTime(c.occurredAt)}>
          {formatRelative(c.occurredAt)}
        </span>
      </button>
      {open && (
        <div className="space-y-2 px-10 pb-3 text-xs">
          {c.exception && (
            <p className="text-muted">
              Root cause: <span className="selectable font-mono text-fg">{c.exception}</span>
            </p>
          )}
          {c.suspects.length > 0 && (
            <p className="text-muted">
              {c.suspects.length === 1 ? "This plugin/mod" : "These plugins/mods"} ran in the crashing code:{" "}
              {c.suspects.map((s) => s.name).join(", ")}. Check for an update, or disable {c.suspects.length === 1 ? "it" : "them"} to test.
            </p>
          )}
          <p className="text-muted">
            Exit code {c.exitCode ?? "unknown"}
            {c.crashReport && (
              <>
                {" "}
                · crash report <span className="selectable font-mono text-fg">{c.crashReport}</span>
              </>
            )}
          </p>
          <pre className="selectable max-h-56 overflow-auto rounded-md bg-surface-3 p-2 font-mono text-[11px] leading-relaxed text-fg">
            {c.consoleTail.join("\n")}
          </pre>
        </div>
      )}
    </li>
  );
}

export function CrashHistory({ serverId }: { serverId: string }) {
  const { data } = useCrashes(serverId);
  if (!data || data.length === 0) return null;
  return (
    <Card>
      <CardHeader title="Recent crashes" description="Newest first. Expand an entry for the console output before the crash." />
      <ul>
        {data.map((c) => (
          <Row key={c.id} c={c} />
        ))}
      </ul>
    </Card>
  );
}
