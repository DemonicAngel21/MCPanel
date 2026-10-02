import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDownToLine, Clock, Copy, Download, Eraser, Pause, Play, Search, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { toast } from "sonner";
import type { ConsoleLineDto } from "@/bindings/ConsoleLineDto";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Input, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatTime } from "@/lib/format";
import { parseMinecraftText, stripFormatting } from "@/lib/minecraft-text";
import { useServer } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

const MAX_LINES = 20_000;
const ROW = 18;

type Line = ConsoleLineDto | { seq: number; at: number; stream: "gap"; level: null; text: string };

const QUICK_COMMANDS = [
  { label: "tps", cmd: "tps", tip: "Check ticks per second" },
  { label: "mspt", cmd: "mspt", tip: "Check tick duration" },
  { label: "list", cmd: "list", tip: "List online players" },
  { label: "save-all", cmd: "save-all", tip: "Save world to disk" },
  { label: "day", cmd: "time set day", tip: "Set time to day" },
  { label: "clear weather", cmd: "weather clear", tip: "Clear bad weather" },
  { label: "reload", cmd: "reload", tip: "Reload server configuration" },
];

/** Built-in suggestions (vanilla commands); not queried from the server. */
const COMMANDS = [
  "help",
  "list",
  "say ",
  "tell ",
  "me ",
  "op ",
  "deop ",
  "kick ",
  "ban ",
  "ban-ip ",
  "pardon ",
  "pardon-ip ",
  "banlist",
  "whitelist on",
  "whitelist off",
  "whitelist add ",
  "whitelist remove ",
  "whitelist list",
  "whitelist reload",
  "gamemode survival ",
  "gamemode creative ",
  "gamemode adventure ",
  "gamemode spectator ",
  "defaultgamemode ",
  "difficulty peaceful",
  "difficulty easy",
  "difficulty normal",
  "difficulty hard",
  "time set day",
  "time set night",
  "time query daytime",
  "weather clear",
  "weather rain",
  "weather thunder",
  "tp ",
  "give ",
  "clear ",
  "effect give ",
  "enchant ",
  "xp add ",
  "kill ",
  "gamerule ",
  "seed",
  "setworldspawn",
  "spawnpoint ",
  "save-all",
  "save-all flush",
  "save-on",
  "save-off",
  "reload",
  "stop",
  "tick query",
  "tps",
  "mspt",
  "plugins",
  "version",
];

const LEVEL_CLASS: Record<string, string> = {
  warn: "text-warning",
  error: "text-danger",
  fatal: "text-danger font-semibold",
};

function LineView({ line, showTime, highlight }: { line: Line; showTime: boolean; highlight: string }) {
  if (line.stream === "gap") {
    return <span className="text-faint italic">{line.text}</span>;
  }
  const cls =
    line.stream === "system"
      ? "text-info"
      : line.stream === "command"
        ? "text-accent"
        : line.level
          ? (LEVEL_CLASS[line.level] ?? "")
          : line.stream === "stderr"
            ? "text-danger"
            : "";
  const segments = line.stream === "system" || line.stream === "command" ? [{ text: line.text }] : parseMinecraftText(line.text);
  return (
    <span className={cls}>
      {showTime && <span className="mr-2 text-faint select-none">{formatTime(line.at)}</span>}
      {line.stream === "command" && <span className="select-none">&gt; </span>}
      {line.stream === "system" && <span className="select-none">[MCPanel] </span>}
      {segments.map((s, i) => {
        const style = {
          color: cls ? undefined : s.color,
          fontWeight: s.bold ? 600 : undefined,
          fontStyle: s.italic ? "italic" : undefined,
          textDecoration: [s.underline && "underline", s.strike && "line-through"].filter(Boolean).join(" ") || undefined,
        };
        if (!highlight)
          return (
            <span key={i} style={style}>
              {s.text}
            </span>
          );
        const parts = s.text.split(new RegExp(`(${highlight.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi"));
        return (
          <span key={i} style={style}>
            {parts.map((p, j) =>
              j % 2 === 1 ? (
                <mark key={j} className="rounded-sm bg-warning/40 text-fg">
                  {p}
                </mark>
              ) : (
                p
              ),
            )}
          </span>
        );
      })}
    </span>
  );
}

export function ServerConsole() {
  const id = useServerId();
  const { data: server } = useServer(id);
  const lines = useRef<Line[]>([]);
  const pending = useRef<Line[]>([]);
  const [version, setVersion] = useState(0);
  const [paused, setPaused] = useState(false);
  const pausedRef = useRef(false);
  const [stick, setStick] = useState(true);
  const [showTime, setShowTime] = useState(false);
  const [filter, setFilter] = useState("");
  const [level, setLevel] = useState("all");
  const [clearedSeq, setClearedSeq] = useState(0);
  const [input, setInput] = useState("");
  const history = useRef<string[]>([]);
  const historyIdx = useRef(-1);
  const scrollRef = useRef<HTMLDivElement>(null);
  const frame = useRef<number | null>(null);

  const schedule = useCallback(() => {
    if (frame.current != null) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      setVersion((v) => v + 1);
    });
  }, []);

  const append = useCallback(
    (incoming: Line[]) => {
      const target = pausedRef.current ? pending.current : lines.current;
      target.push(...incoming);
      if (target.length > MAX_LINES) target.splice(0, target.length - MAX_LINES);
      if (!pausedRef.current) schedule();
      else setVersion((v) => v + 1);
    },
    [schedule],
  );

  useEffect(() => {
    lines.current = [];
    pending.current = [];
    setVersion((v) => v + 1);
    let unsub: (() => void) | undefined;
    let cancelled = false;
    void api.console
      .subscribe(id, null, 5000, (batch) => {
        const out: Line[] = [];
        if (batch.gapFrom != null && batch.gapTo != null) {
          out.push({
            seq: -batch.gapFrom,
            at: Date.now(),
            stream: "gap",
            level: null,
            text: `… ${batch.gapTo - batch.gapFrom + 1} lines were skipped because output was faster than the display`,
          });
        }
        out.push(...batch.lines);
        append(out);
      })
      .then((u) => {
        if (cancelled) u();
        else unsub = u;
      })
      .catch((e) => toast.error(errorMessage(e)));
    return () => {
      cancelled = true;
      unsub?.();
      if (frame.current != null) cancelAnimationFrame(frame.current);
      frame.current = null;
    };
  }, [id, append]);

  const visible = useMemo(() => {
    void version;
    const f = filter.trim().toLowerCase();
    const minLevel = level === "warn" ? ["warn", "error", "fatal"] : level === "error" ? ["error", "fatal"] : null;
    return lines.current.filter((l) => {
      if (l.stream !== "gap" && l.seq <= clearedSeq) return false;
      if (minLevel && !(l.level && minLevel.includes(l.level)) && l.stream !== "stderr") return false;
      if (f && !stripFormatting(l.text).toLowerCase().includes(f)) return false;
      return true;
    });
  }, [version, filter, level, clearedSeq]);

  // TanStack Virtual returns non-memoizable functions; the React Compiler skips this component.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virt = useVirtualizer({
    count: visible.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW,
    overscan: 30,
  });

  useEffect(() => {
    if (stick && visible.length > 0) virt.scrollToIndex(visible.length - 1, { align: "end" });
  }, [visible.length, stick, virt]);

  // Only user input may leave follow mode; content growth also fires scroll events.
  const userScrolling = useRef(false);
  const atBottom = () => {
    const el = scrollRef.current;
    return !el || el.scrollHeight - el.scrollTop - el.clientHeight < ROW * 2;
  };
  const intentTimer = useRef<number | null>(null);
  const onUserScrollIntent = () => {
    userScrolling.current = true;
    if (intentTimer.current) window.clearTimeout(intentTimer.current);
    intentTimer.current = window.setTimeout(() => (userScrolling.current = false), 250);
  };
  const onScroll = () => {
    if (atBottom()) {
      if (!stick) setStick(true);
    } else if (userScrolling.current && stick) {
      setStick(false);
    }
  };

  const togglePause = () => {
    const next = !paused;
    pausedRef.current = next;
    setPaused(next);
    if (!next && pending.current.length) {
      lines.current.push(...pending.current);
      pending.current = [];
      if (lines.current.length > MAX_LINES) lines.current.splice(0, lines.current.length - MAX_LINES);
      schedule();
    }
  };

  const canSend = server?.state === "running" || server?.state === "starting";
  const sendCommand = async (cmdToSend: string) => {
    const cmd = cmdToSend.trim();
    if (!cmd) return;
    try {
      await api.servers.command(id, cmd);
      history.current = [cmd, ...history.current.filter((h) => h !== cmd)].slice(0, 100);
      historyIdx.current = -1;
      setStick(true);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const send = async () => {
    await sendCommand(input);
    setInput("");
  };

  const suggestion = useMemo(() => {
    const v = input.trimStart().replace(/^\//, "");
    if (!v || v.includes("  ")) return null;
    return COMMANDS.find((c) => c.startsWith(v) && c !== v) ?? null;
  }, [input]);

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void send();
    } else if (e.key === "Tab" && suggestion) {
      e.preventDefault();
      setInput(suggestion);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      const next = Math.min(historyIdx.current + 1, history.current.length - 1);
      if (next >= 0) {
        historyIdx.current = next;
        setInput(history.current[next] ?? "");
      }
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      const next = historyIdx.current - 1;
      historyIdx.current = Math.max(next, -1);
      setInput(next >= 0 ? (history.current[next] ?? "") : "");
    }
  };

  const copy = async () => {
    const text = visible.map((l) => stripFormatting(l.text)).join("\n");
    await navigator.clipboard.writeText(text);
    toast.success(`Copied ${visible.length} lines`);
  };

  const download = async () => {
    try {
      const g = await api.dialog.saveFile(`${server?.name ?? "server"}-console.log`);
      if (!g) return;
      const n = await api.console.export(id, g.token);
      toast.success(`Saved ${n} lines to ${g.name}`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col px-6 py-4">
      <div className="mb-2 flex items-center gap-2">
        <div className="relative w-64">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-faint" />
          <Input
            className="pr-7 pl-8"
            placeholder="Filter output"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setFilter("");
            }}
          />
          {filter && (
            <button
              type="button"
              onClick={() => setFilter("")}
              className="absolute top-1/2 right-2 -translate-y-1/2 text-muted hover:text-fg"
              aria-label="Clear filter"
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>
        <Select
          aria-label="Log level"
          className="w-36"
          value={level}
          onValueChange={setLevel}
          options={[
            { value: "all", label: "All levels" },
            { value: "warn", label: "Warnings+" },
            { value: "error", label: "Errors" },
          ]}
        />
        <span className="text-xs text-muted tabular-nums">
          {filter.trim() || level !== "all" ? `${visible.length} / ${lines.current.length} lines` : `${lines.current.length} lines`}
        </span>
        <div className="flex-1" />
        <Tooltip content={showTime ? "Hide receive times" : "Show receive times"}>
          <Button variant={showTime ? "secondary" : "ghost"} size="icon-sm" onClick={() => setShowTime((v) => !v)} aria-label="Timestamps">
            <Clock />
          </Button>
        </Tooltip>
        <Tooltip content={paused ? "Resume" : "Pause output"}>
          <Button variant={paused ? "secondary" : "ghost"} size="icon-sm" onClick={togglePause} aria-label="Pause">
            {paused ? <Play /> : <Pause />}
          </Button>
        </Tooltip>
        <Tooltip content="Clear view (does not delete logs)">
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setClearedSeq(lines.current.reduce((m, l) => Math.max(m, l.seq), 0))}
            aria-label="Clear"
          >
            <Eraser />
          </Button>
        </Tooltip>
        <Tooltip content="Copy visible lines">
          <Button variant="ghost" size="icon-sm" onClick={copy} aria-label="Copy">
            <Copy />
          </Button>
        </Tooltip>
        <Tooltip content="Save console log to a file">
          <Button variant="ghost" size="icon-sm" onClick={download} aria-label="Download">
            <Download />
          </Button>
        </Tooltip>
      </div>

      <div className="relative min-h-0 flex-1 overflow-hidden rounded-lg border border-border bg-console">
        <div
          ref={scrollRef}
          onScroll={onScroll}
          onWheel={onUserScrollIntent}
          onPointerDown={() => {
            if (intentTimer.current) window.clearTimeout(intentTimer.current);
            userScrolling.current = true;
          }}
          onPointerUp={onUserScrollIntent}
          onKeyDown={onUserScrollIntent}
          tabIndex={0}
          className="selectable h-full overflow-auto py-2 font-mono text-[12px] leading-[18px] text-fg/90"
        >
          <div style={{ height: virt.getTotalSize(), position: "relative", minWidth: "100%", width: "max-content" }}>
            {virt.getVirtualItems().map((item) => {
              const line = visible[item.index];
              if (!line) return null;
              return (
                <div key={item.key} className="absolute left-0 w-full px-3 whitespace-pre" style={{ top: item.start, height: ROW }}>
                  <LineView line={line} showTime={showTime} highlight={filter.trim()} />
                </div>
              );
            })}
          </div>
          {visible.length === 0 && (
            <p className="px-3 text-faint">
              {filter || level !== "all" ? "No matching lines." : "No output yet. Start the server to see its console."}
            </p>
          )}
        </div>
        {paused && pending.current.length > 0 && (
          <div className="absolute top-2 left-1/2 -translate-x-1/2 rounded-full bg-warning px-3 py-1 text-xs font-medium text-black shadow">
            Paused · {pending.current.length} new lines
          </div>
        )}
        {!stick && (
          <Button variant="secondary" size="sm" className="absolute right-4 bottom-3 shadow-lg" onClick={() => setStick(true)}>
            <ArrowDownToLine /> Jump to latest
          </Button>
        )}
      </div>

      {canSend && (
        <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs">
          <span className="text-[11px] font-medium text-faint">Quick:</span>
          {QUICK_COMMANDS.map((qc) => (
            <Tooltip key={qc.label} content={qc.tip}>
              <button
                type="button"
                onClick={() => void sendCommand(qc.cmd)}
                className="rounded border border-border/80 bg-surface-2 px-2 py-0.5 font-mono text-[11px] text-muted transition-colors hover:border-accent hover:text-fg"
              >
                {qc.label}
              </button>
            </Tooltip>
          ))}
        </div>
      )}

      <div className="relative mt-2">
        <span className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 font-mono text-[12px] text-accent">&gt;</span>
        {suggestion && canSend && (
          <span className="pointer-events-none absolute top-1/2 left-7 -translate-y-1/2 font-mono text-[12px] whitespace-pre text-faint">
            <span className="invisible">{input.trimStart().replace(/^\//, "")}</span>
            {suggestion.slice(input.trimStart().replace(/^\//, "").length)}
            <span className="ml-2 text-[10px]">Tab</span>
          </span>
        )}
        <Input
          className="h-9 pl-7 font-mono text-[12px]"
          placeholder={canSend ? "Type a command (↑↓ history, Tab to complete)" : "Start the server to send commands"}
          value={input}
          disabled={!canSend}
          onChange={(e) => {
            setInput(e.target.value);
            historyIdx.current = -1;
          }}
          onKeyDown={onKey}
          autoFocus
        />
      </div>
    </div>
  );
}
