import { useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ArrowRight, Check, Copy, Download, ExternalLink, Globe, Link2, Play, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import type { TunnelStatusDto } from "@/bindings/TunnelStatusDto";
import { Button } from "@/components/ui/button";
import { Badge, Card, CardHeader, Input, Spinner, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, usePlayitAgent, usePlayitTunnels, useTunnel, useTunnelAddress } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

type Tone = "neutral" | "success" | "warning" | "danger" | "info";

/** What the agent is doing, in words (phases from `playit status`, playit 1.0). */
function agentState(t: TunnelStatusDto): { tone: Tone; label: string } {
  if (!t.installed) return { tone: "neutral", label: "Not installed" };
  if (t.linkState === "waiting") return { tone: "info", label: "Linking…" };
  if (t.agentRunning === false) return { tone: "neutral", label: "Agent stopped" };
  if (t.agentRunning == null) return { tone: "warning", label: "State unknown" };
  switch (t.phase) {
    case "running":
      return { tone: "success", label: "Online" };
    case "starting":
    case "stopping":
      return { tone: "info", label: t.phase === "starting" ? "Starting…" : "Stopping…" };
    case "waiting for secret":
      return { tone: "warning", label: "Not linked" };
    case "invalid secret":
      return { tone: "danger", label: "Link invalid" };
    case "disabled over limit":
      return { tone: "danger", label: "Over plan limit" };
    case "error":
      return { tone: "danger", label: "Agent error" };
    default:
      return { tone: "success", label: t.phase ? `Agent ${t.phase}` : "Agent running" };
  }
}

const open = (url: string) => api.app.openExternal(url).catch((e) => toast.error(errorMessage(e)));

/** Start/stop/link actions with shared busy state and link-outcome toasts. */
function useServiceAgent() {
  const qc = useQueryClient();
  const { data: t } = useTunnel();
  const [busy, setBusy] = useState<null | "start" | "stop" | "link" | "install">(null);
  const linkState = t?.linkState;

  // Report the outcome of a link started here once.
  const seenLink = useRef<string | null>(null);
  useEffect(() => {
    if (!linkState || linkState === seenLink.current) return;
    const first = seenLink.current === null && linkState !== "waiting";
    seenLink.current = linkState;
    if (first) return; // an outcome from before this page was shown
    if (linkState === "linked") toast.success("playit agent linked to your account");
    if (linkState === "failed" && t?.linkError && t.linkError !== "Cancelled") toast.error(t.linkError);
  }, [linkState, t?.linkError]);

  const run = async (kind: "start" | "stop" | "link" | "install", fn: () => Promise<unknown>) => {
    setBusy(kind);
    try {
      const r = await fn();
      if (r && typeof r === "object") qc.setQueryData(qk.tunnel, r);
      else await qc.invalidateQueries({ queryKey: qk.tunnel });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  return {
    t,
    busy,
    install: () => run("install", api.tunnels.installAgent),
    start: () => run("start", api.tunnels.startAgent),
    stop: () => run("stop", api.tunnels.stopAgent),
    link: () => run("link", api.tunnels.link),
    cancelLink: () => run("link", api.tunnels.cancelLink),
  };
}

/** The playit agent: state, version, start/stop and account linking (official program). */
export function PlayitAgentCard() {
  const { t, busy, install, start, stop, link, cancelLink } = useServiceAgent();
  if (!t) return <Card className="h-28 animate-skeleton" />;
  const state = agentState(t);
  return (
    <Card>
      <CardHeader
        title={
          <span className="flex items-center gap-2">
            Installed playit service
            {t.version && <span className="font-mono text-[11px] font-normal text-faint">{t.version}</span>}
            <Badge tone={state.tone}>{state.label}</Badge>
          </span>
        }
        description="The playit program's own background service and its tunnels (separate from MCPanel's agent). MCPanel can start and stop it."
      />
      <div className="p-4">
        {!t.installed ? (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-xs text-muted">The playit program is not installed. Install it automatically or download it from its official site.</p>
            <div className="flex items-center gap-2">
              <Button size="sm" variant="primary" disabled={busy != null} onClick={install}>
                {busy === "install" ? <Spinner /> : <Download />} Install Playit agent
              </Button>
              <Button size="sm" variant="outline" onClick={() => open("https://playit.gg/download")}>
                Website <ExternalLink />
              </Button>
            </div>
          </div>
        ) : (
          <AgentRow t={t} busy={busy} onStart={start} onStop={stop} onLink={link} onCancelLink={cancelLink} />
        )}
      </div>
    </Card>
  );
}

/** One line for a server's Overview: its public address, with a way to the playit section. */
export function InternetAccessSummary({ serverId, port }: { serverId: string; port: number }) {
  const { data: t } = useTunnel();
  const { data: manual } = useTunnelAddress(serverId);
  const { data: agent } = usePlayitAgent();
  const { data: list } = usePlayitTunnels(!!agent?.linked);
  // A tunnel of the account to this server's port wins over an address typed in by hand.
  const tunnel = list?.tunnels.find((x) => x.localPort === port && x.tunnelType !== "minecraft-bedrock" && x.addresses.length > 0);
  const saved = tunnel?.addresses[0] ?? manual ?? null;
  const state = agent?.linked
    ? agent.running
      ? { tone: "success" as const, label: "Agent online" }
      : { tone: "neutral" as const, label: "Agent stopped" }
    : t
      ? agentState(t)
      : null;
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border px-3 py-2.5">
      <div className="min-w-0 space-y-1">
        <p className="flex items-center gap-2 text-[13px] font-medium text-fg">
          <Globe className="size-4 text-muted" /> Internet access
          {state && <Badge tone={state.tone}>{state.label}</Badge>}
        </p>
        {saved ? (
          <span className="flex items-center gap-1.5">
            <code className="selectable font-mono text-[13px] text-fg">{saved}</code>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Copy public address"
              onClick={() => navigator.clipboard.writeText(saved).then(() => toast.success("Address copied"))}
            >
              <Copy />
            </Button>
          </span>
        ) : (
          <p className="text-xs text-muted">
            No playit.gg address yet. Tunnel to <code className="font-mono text-fg">127.0.0.1:{port}</code> to let friends outside your network join.
          </p>
        )}
      </div>
      <Button asChild size="sm" variant="outline">
        <Link to="/playit">
          Playit.gg <ArrowRight />
        </Link>
      </Button>
    </div>
  );
}

function AgentRow({
  t,
  busy,
  onStart,
  onStop,
  onLink,
  onCancelLink,
}: {
  t: TunnelStatusDto;
  busy: null | "start" | "stop" | "link" | "install";
  onStart: () => void;
  onStop: () => void;
  onLink: () => void;
  onCancelLink: () => void;
}) {
  if (t.linkState === "waiting" && t.linkClaimUrl) {
    const url = t.linkClaimUrl;
    return (
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-md bg-info-soft px-3 py-2">
        <span className="flex items-center gap-2 text-xs text-fg">
          <Spinner /> Approve the agent on playit.gg in your browser…
        </span>
        <span className="flex gap-1.5">
          <Button size="sm" variant="ghost" onClick={() => open(url)}>
            Open again <ExternalLink />
          </Button>
          <Button size="sm" variant="ghost" onClick={onCancelLink} disabled={busy != null}>
            Cancel
          </Button>
        </span>
      </div>
    );
  }
  const running = t.agentRunning === true;
  const description = !running
    ? t.agentRunning === false
      ? "The agent is stopped, so your tunnels are offline."
      : "MCPanel could not read the agent's state."
    : t.canLink
      ? "The agent runs but is not linked to a playit.gg account yet."
      : t.phase === "invalid secret"
        ? "The agent's link was rejected by playit.gg. Reset it with the playit program (playit reset) and link it again."
        : "The agent is online; tunnels of this agent are reachable.";
  return (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <p className="text-xs text-muted">{description}</p>
      <span className="flex gap-1.5">
        {t.canLink && (
          <Button size="sm" variant="primary" onClick={onLink} disabled={busy != null}>
            {busy === "link" ? <Spinner /> : <Link2 />} Link account
          </Button>
        )}
        {t.canControlAgent &&
          (running ? (
            <Tooltip content="Stops the playit service: all tunnels of this agent go offline.">
              <Button
                size="sm"
                variant="outline"
                className="transition-colors hover:border-danger hover:bg-danger hover:text-white [&:hover_svg]:text-white"
                onClick={onStop}
                disabled={busy != null}
              >
                {busy === "stop" ? <Spinner /> : <Square />} Stop agent
              </Button>
            </Tooltip>
          ) : (
            <Tooltip content="Starts the playit service: all tunnels of this agent go online.">
              <Button size="sm" variant="outline" onClick={onStart} disabled={busy != null}>
                {busy === "start" ? <Spinner /> : <Play />} Start agent
              </Button>
            </Tooltip>
          ))}
      </span>
    </div>
  );
}

/** The public address saved for a server, with the steps to get one. */
export function ServerAddressEditor({
  serverId,
  port,
  ready,
  steps = true,
}: {
  serverId: string;
  port: number;
  ready: boolean;
  /** Show the setup steps (off where the page already explains them). */
  steps?: boolean;
}) {
  const qc = useQueryClient();
  const { data: saved, isLoading } = useTunnelAddress(serverId);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const local = `127.0.0.1:${port}`;

  const save = async () => {
    try {
      const v = await api.tunnels.setServerAddress(serverId, draft);
      qc.setQueryData(["tunnel", "address", serverId], v);
      setEditing(false);
      setError(null);
      toast.success(v ? "Public address saved" : "Public address removed");
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  if (isLoading) return null;

  if (saved && !editing) {
    return (
      <div className="space-y-1.5">
        <p className="text-xs text-muted">Public address {ready ? "" : "(online while the agent runs)"}</p>
        <div className="flex flex-wrap items-center gap-2">
          <code className="selectable rounded-md border border-border bg-surface-2 px-2.5 py-1.5 font-mono text-[13px] text-fg">{saved}</code>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Copy public address"
            onClick={() => navigator.clipboard.writeText(saved).then(() => toast.success("Address copied"))}
          >
            <Copy />
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              setDraft(saved);
              setEditing(true);
            }}
          >
            Change
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {steps && (
        <ol className="list-decimal space-y-0.5 pl-4 text-xs text-muted">
          <li>
            On playit.gg, add a <span className="text-fg">Minecraft Java</span> tunnel for this agent with local address{" "}
            <code className="selectable font-mono text-fg">{local}</code> (and a Minecraft Bedrock tunnel if you use Geyser).
          </li>
          <li>Paste the tunnel's public address here so MCPanel can show it with this server.</li>
        </ol>
      )}
      <div className="flex flex-wrap items-start gap-2">
        <Button size="sm" variant="outline" onClick={() => open("https://playit.gg/account/tunnels")}>
          Tunnels on playit.gg <ExternalLink />
        </Button>
        <form
          className="flex min-w-60 flex-1 gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <Input
            aria-label="Public address"
            placeholder="name.joinmc.link"
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              setError(null);
            }}
            className="h-7 flex-1 font-mono text-xs"
            spellCheck={false}
          />
          <Button size="sm" type="submit" disabled={!draft.trim() && !saved}>
            <Check /> Save
          </Button>
          {editing && (
            <Button size="sm" variant="ghost" type="button" onClick={() => setEditing(false)}>
              Cancel
            </Button>
          )}
        </form>
      </div>
      {error && <p className="text-xs text-danger">{error}</p>}
    </div>
  );
}
