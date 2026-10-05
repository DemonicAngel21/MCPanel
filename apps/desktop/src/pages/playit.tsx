import { useQueryClient } from "@tanstack/react-query";
import { Copy, Download, ExternalLink, Globe, Link2, MoreHorizontal, Pencil, Play, Plus, Square, Trash2, Unlink } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import type { PlayitAgentDto } from "@/bindings/PlayitAgentDto";
import type { PlayitTunnelDto } from "@/bindings/PlayitTunnelDto";
import type { ServerDto } from "@/bindings/ServerDto";
import { PageBody, PageHeader } from "@/app/app-shell";
import { PlayitAgentCard } from "@/components/playit-card";
import { Button } from "@/components/ui/button";
import {
  ConfirmDialog,
  Dialog,
  DialogContent,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  Select,
} from "@/components/ui/overlays";
import { Badge, Card, CardHeader, EmptyState, Field, Input, SkeletonRows, Spinner, Switch, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, usePlayitAgent, usePlayitTunnels, useServers } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

const open = (url: string) => api.app.openExternal(url).catch((e) => toast.error(errorMessage(e)));

type Tone = "neutral" | "success" | "warning" | "danger" | "info";

function agentBadge(a: PlayitAgentDto): { tone: Tone; label: string } {
  if (!a.installed) return { tone: "neutral", label: "playit not installed" };
  if (a.linkState === "waiting") return { tone: "info", label: "Linking…" };
  if (!a.linked) return { tone: "warning", label: "Not linked" };
  if (!a.running) return { tone: "neutral", label: "Stopped" };
  if (a.phase === "invalid secret") return { tone: "danger", label: "Link rejected" };
  if (a.phase === "disabled over limit") return { tone: "danger", label: "Over plan limit" };
  if (a.phase === "running") return { tone: "success", label: "Online" };
  return { tone: "info", label: a.phase ? `${a.phase[0]?.toUpperCase()}${a.phase.slice(1)}` : "Starting…" };
}

/** MCPanel's own playit agent: link, start/stop, autostart, unlink. */
function McpanelAgentCard({ agent }: { agent: PlayitAgentDto }) {
  const qc = useQueryClient();
  const [busy, setBusy] = useState<string | null>(null);
  const [confirmUnlink, setConfirmUnlink] = useState(false);
  const [confirmRelink, setConfirmRelink] = useState(false);
  const badge = agentBadge(agent);

  // Report a link started here once.
  const seen = useRef<string | null>(null);
  useEffect(() => {
    const st = agent.linkState;
    if (!st || st === seen.current) return;
    const first = seen.current === null && st !== "waiting";
    seen.current = st;
    if (first) return;
    if (st === "linked") {
      toast.success("MCPanel's playit agent is linked to your account");
      void qc.invalidateQueries({ queryKey: qk.playitTunnels });
    }
    if (st === "failed" && agent.linkError) toast.error(agent.linkError);
  }, [agent.linkState, agent.linkError, qc]);

  const run = async (kind: string, fn: () => Promise<unknown>) => {
    setBusy(kind);
    try {
      const r = await fn();
      if (r && typeof r === "object") qc.setQueryData(qk.playit, r);
      else await qc.invalidateQueries({ queryKey: qk.playit });
      void qc.invalidateQueries({ queryKey: qk.playitTunnels });
    } catch (e) {
      toast.error(errorMessage(e));
      if (kind === "link" || kind === "relink") {
        void qc.invalidateQueries({ queryKey: qk.playit });
      }
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card>
      <CardHeader
        title={
          <span className="flex items-center gap-2">
            MCPanel's playit agent <Badge tone={badge.tone}>{badge.label}</Badge>
          </span>
        }
        description="MCPanel links its own agent to your playit.gg account and runs it while MCPanel is open, so it can create and manage tunnels for your servers."
      />
      <div className="space-y-3 p-4">
        {!agent.installed ? (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-xs text-muted">
              The playit program is needed to run the agent. Install it automatically or download it from its official site.
            </p>
            <div className="flex items-center gap-2">
              <Button size="sm" variant="primary" disabled={busy != null} onClick={() => run("install", api.playit.installAgent)}>
                {busy === "install" ? <Spinner /> : <Download />} Install Playit agent
              </Button>
              <Button size="sm" variant="outline" onClick={() => open("https://playit.gg/download")}>
                Website <ExternalLink />
              </Button>
            </div>
          </div>
        ) : agent.linkState === "waiting" && agent.linkUrl ? (
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-md bg-info-soft px-3 py-2.5">
            <span className="flex items-center gap-2 text-xs text-fg">
              <Spinner /> Approve MCPanel's agent on playit.gg in your browser (sign in or create a free account there)…
            </span>
            <span className="flex gap-1.5">
              <Button size="sm" variant="ghost" onClick={() => open(agent.linkUrl!)}>
                Open again <ExternalLink />
              </Button>
              <Button size="sm" variant="ghost" onClick={() => run("cancel", api.playit.cancelLink)}>
                Cancel
              </Button>
            </span>
          </div>
        ) : !agent.linked ? (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="max-w-2xl text-xs text-muted">
              Link once: playit.gg opens in your browser, you approve the agent, and MCPanel keeps its key in the Windows Credential Manager. Your
              other playit agents and tunnels are not changed.
            </p>
            <Button variant="primary" onClick={() => run("link", api.playit.link)} disabled={busy != null}>
              {busy === "link" ? <Spinner /> : <Link2 />} Link with playit.gg
            </Button>
          </div>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="space-y-1">
              <p className="text-xs text-muted">
                {agent.phase === "invalid secret"
                  ? "playit.gg rejected the agent's key (it may have been removed on playit.gg). Unlink and link again."
                  : agent.running
                    ? "Tunnels of this agent are reachable while MCPanel runs."
                    : "The agent is stopped, so its tunnels are offline."}
              </p>
              <label className="flex items-center gap-2 text-xs text-muted">
                <Switch checked={agent.autostart} onCheckedChange={(on) => run("autostart", () => api.playit.setAutostart(on))} />
                Start the agent when MCPanel starts
              </label>
            </div>
            <span className="flex gap-1.5">
              {agent.running ? (
                <Button
                  variant="outline"
                  className="transition-colors hover:border-danger hover:bg-danger hover:text-white [&:hover_svg]:text-white"
                  onClick={() => run("stop", api.playit.stop)}
                  disabled={busy != null}
                >
                  {busy === "stop" ? <Spinner /> : <Square />} Stop
                </Button>
              ) : (
                <Button variant="primary" onClick={() => run("start", api.playit.start)} disabled={busy != null}>
                  {busy === "start" ? <Spinner /> : <Play />} Start
                </Button>
              )}
              <Button variant="ghost" onClick={() => setConfirmUnlink(true)} disabled={busy != null}>
                <Unlink /> Unlink
              </Button>
              <Button variant="ghost" onClick={() => setConfirmRelink(true)} disabled={busy != null}>
                <Link2 /> Relink
              </Button>
            </span>
          </div>
        )}
      </div>
      <ConfirmDialog
        open={confirmUnlink}
        onOpenChange={setConfirmUnlink}
        title="Unlink MCPanel's playit agent?"
        description="MCPanel stops the agent and forgets its key. Its tunnels go offline. The agent stays listed on playit.gg until you remove it there."
        confirmLabel="Unlink"
        destructive
        onConfirm={() => run("unlink", api.playit.unlink)}
      />
      <ConfirmDialog
        open={confirmRelink}
        onOpenChange={setConfirmRelink}
        title="Relink MCPanel's playit agent?"
        description="MCPanel stops its current agent, forgets its saved key, and opens a new approval link. The current agent and its tunnels may remain listed on playit.gg."
        confirmLabel="Relink"
        destructive
        onConfirm={() => run("relink", api.playit.relink)}
      />
    </Card>
  );
}

function tunnelState(t: PlayitTunnelDto, agentRunning: boolean): { tone: Tone; label: string; hint?: string } {
  if (!t.enabled) return { tone: "neutral", label: "Disabled" };
  if (t.offlineReasons.includes("PublicAllocationPending")) return { tone: "info", label: "Getting address…" };
  if (t.offlineReasons.length > 0) return { tone: "warning", label: "Offline", hint: t.offlineReasons.join(", ") };
  if (t.editable && !agentRunning) return { tone: "neutral", label: "Agent stopped" };
  return { tone: "success", label: "Online" };
}

function kindLabel(t: PlayitTunnelDto) {
  return t.tunnelType === "minecraft-java" ? "Java" : t.tunnelType === "minecraft-bedrock" ? "Bedrock" : (t.tunnelType ?? t.portType.toUpperCase());
}

/** Create or edit a tunnel: server (or custom port), type and name. */
function TunnelDialog({ servers, editing, onClose }: { servers: ServerDto[]; editing: PlayitTunnelDto | null; onClose: () => void }) {
  const qc = useQueryClient();
  const matched = editing ? servers.find((s) => (s.port ?? 25565) === editing.localPort) : servers[0];
  const [serverId, setServerId] = useState<string>(matched?.id ?? "custom");
  const [kind, setKind] = useState<string>(editing?.tunnelType === "minecraft-bedrock" ? "minecraft-bedrock" : "minecraft-java");
  const server = servers.find((s) => s.id === serverId);
  const defaultPort = kind === "minecraft-bedrock" ? 19132 : (server?.port ?? 25565);
  const [port, setPort] = useState<string>(String(editing?.localPort ?? defaultPort));
  const [name, setName] = useState(editing?.name ?? (server ? `${server.name} ${kind === "minecraft-bedrock" ? "Bedrock" : "Java"}` : "Minecraft"));
  const [busy, setBusy] = useState(false);
  const ascii = /^[\x20-\x7e]*$/.test(name);

  const pick = async (id: string, k = kind) => {
    setServerId(id);
    const s = servers.find((x) => x.id === id);
    if (s && k === "minecraft-java") setPort(String(s.port ?? 25565));
    if (k === "minecraft-bedrock") {
      let bp = 19132;
      if (s) {
        try {
          const status = await api.bedrock.status(s.id);
          bp = status.activePort ?? status.settings.port ?? 19132;
        } catch {
          // Keep default if status unavailable
        }
      }
      setPort(String(bp));
    }
    if (!editing && s) setName(`${s.name} ${k === "minecraft-bedrock" ? "Bedrock" : "Java"}`.replace(/[^\x20-\x7e]/g, "").slice(0, 64));
  };

  const save = async () => {
    const p = Number(port);
    setBusy(true);
    try {
      if (editing) {
        if (name.trim() !== (editing.name ?? "")) await api.playit.renameTunnel(editing.id, name);
        if (p !== editing.localPort) await api.playit.setTunnelPort(editing.id, p);
        toast.success("Tunnel updated");
      } else {
        await api.playit.createTunnel(name, kind, p);
        toast.success("Tunnel created. playit.gg assigns its address in a moment.");
      }
      await qc.invalidateQueries({ queryKey: qk.playitTunnels });
      onClose();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const portOk = /^\d{1,5}$/.test(port) && Number(port) >= 1 && Number(port) <= 65535;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={editing ? "Edit tunnel" : "New tunnel"}
        description={
          editing ? "Rename the tunnel or point it at another server." : "playit.gg gives the tunnel a public address that forwards to this computer."
        }
        footer={
          <>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button variant="primary" onClick={() => void save()} disabled={busy || !portOk || !name.trim() || !ascii}>
              {busy && <Spinner className="text-accent-fg" />} {editing ? "Save" : "Create tunnel"}
            </Button>
          </>
        }
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Server">
            <Select
              value={serverId}
              onValueChange={(v) => pick(v)}
              options={[
                ...servers.map((s) => ({ value: s.id, label: s.name, hint: `port ${s.port ?? 25565}` })),
                { value: "custom", label: "Custom port" },
              ]}
            />
          </Field>
          <Field label="Type">
            <Select
              value={kind}
              disabled={!!editing}
              onValueChange={(k) => {
                setKind(k);
                pick(serverId, k);
              }}
              options={[
                { value: "minecraft-java", label: "Minecraft Java (TCP)" },
                { value: "minecraft-bedrock", label: "Minecraft Bedrock (UDP)" },
              ]}
            />
          </Field>
          <Field label="Name" error={!ascii ? "Use only English letters, digits and punctuation." : undefined}>
            <Input value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field
            label="Local port"
            hint={kind === "minecraft-bedrock" ? "Geyser's Bedrock port (Bedrock tab)." : "The server's port on this computer."}
          >
            <Input inputMode="numeric" value={port} onChange={(e) => setPort(e.target.value.replace(/\D/g, "").slice(0, 5))} />
          </Field>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function TunnelsCard({ agent }: { agent: PlayitAgentDto }) {
  const qc = useQueryClient();
  const { data: servers } = useServers();
  const { data, error, isLoading, refetch, isFetching } = usePlayitTunnels(agent.linked);
  const [dialog, setDialog] = useState<{ editing: PlayitTunnelDto | null } | null>(null);
  const [remove, setRemove] = useState<PlayitTunnelDto | null>(null);
  const serverFor = useMemo(() => {
    const map = new Map<number, string>();
    for (const s of servers ?? []) map.set(s.port ?? 25565, s.name);
    return map;
  }, [servers]);

  const toggle = async (t: PlayitTunnelDto, on: boolean) => {
    try {
      await api.playit.setTunnelEnabled(t.id, on);
      await qc.invalidateQueries({ queryKey: qk.playitTunnels });
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <Card>
      <CardHeader
        title={
          <span className="flex items-center gap-2">
            Tunnels
            {data && (
              <Badge tone={data.accountStatus === "verified" ? "success" : "warning"}>
                {data.accountStatus === "verified" ? "Verified account" : data.accountStatus}
              </Badge>
            )}
            {data?.premium && <Badge tone="info">Premium</Badge>}
          </span>
        }
        description="Tunnels on your playit.gg account. MCPanel can change the tunnels of its own agent; others are managed on playit.gg."
        actions={
          <span className="flex gap-1.5">
            <Button size="sm" variant="ghost" onClick={() => void refetch()} disabled={isFetching}>
              {isFetching ? <Spinner /> : null} Refresh
            </Button>
            <Button size="sm" variant="primary" onClick={() => setDialog({ editing: null })} disabled={!data}>
              <Plus /> New tunnel
            </Button>
          </span>
        }
      />
      {isLoading ? (
        <SkeletonRows rows={3} />
      ) : error ? (
        <EmptyState tone="danger" icon={<Globe />} title="Tunnels could not be loaded" description={errorMessage(error)} />
      ) : data && data.tunnels.length === 0 ? (
        <EmptyState
          icon={<Globe />}
          title="No tunnels yet"
          description="Create a tunnel for a server to let friends join from anywhere."
          action={
            <Button variant="primary" size="sm" onClick={() => setDialog({ editing: null })}>
              <Plus /> New tunnel
            </Button>
          }
        />
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-[13px]">
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted">
                <th className="px-4 py-2 font-medium">Name</th>
                <th className="px-4 py-2 font-medium">Public address</th>
                <th className="px-4 py-2 font-medium">Forwards to</th>
                <th className="px-4 py-2 font-medium">Status</th>
                <th className="px-4 py-2 font-medium">
                  <span className="sr-only">Enabled</span>
                </th>
                <th className="w-10 px-2 py-2" />
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {data?.tunnels.map((t) => {
                const st = tunnelState(t, agent.running);
                const addr = t.addresses[0];
                const target = t.localPort != null ? serverFor.get(t.localPort) : undefined;
                return (
                  <tr key={t.id} className="transition-colors duration-100 hover:bg-surface-2">
                    <td className="px-4 py-2.5">
                      <span className="flex items-center gap-2">
                        <span className="font-medium text-fg">{t.name ?? "Unnamed"}</span>
                        <Badge tone={t.tunnelType === "minecraft-bedrock" ? "info" : "neutral"}>{kindLabel(t)}</Badge>
                        {!t.editable && (
                          <Tooltip content="This tunnel belongs to another playit agent; change it on playit.gg.">
                            <span>
                              <Badge tone="neutral">Other agent</Badge>
                            </span>
                          </Tooltip>
                        )}
                      </span>
                    </td>
                    <td className="px-4 py-2.5">
                      {addr ? (
                        <span className="flex items-center gap-1">
                          <code className="selectable font-mono text-xs text-fg">{addr}</code>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            aria-label={`Copy address of ${t.name ?? "tunnel"}`}
                            onClick={() => navigator.clipboard.writeText(addr).then(() => toast.success("Address copied"))}
                          >
                            <Copy />
                          </Button>
                        </span>
                      ) : (
                        <span className="text-xs text-muted">Not assigned yet</span>
                      )}
                    </td>
                    <td className="px-4 py-2.5 text-xs text-muted">
                      {t.localPort != null ? (
                        <>
                          <span className="font-mono">
                            {t.localIp ?? "127.0.0.1"}:{t.localPort}
                          </span>
                          {target && <span className="text-fg"> · {target}</span>}
                        </>
                      ) : (
                        "—"
                      )}
                    </td>
                    <td className="px-4 py-2.5">
                      <Tooltip content={st.hint ?? st.label}>
                        <span>
                          <Badge tone={st.tone}>{st.label}</Badge>
                        </span>
                      </Tooltip>
                    </td>
                    <td className="px-4 py-2.5">
                      <Switch
                        aria-label={`${t.enabled ? "Disable" : "Enable"} ${t.name ?? "tunnel"}`}
                        checked={t.enabled}
                        disabled={!t.editable}
                        onCheckedChange={(on) => void toggle(t, on)}
                      />
                    </td>
                    <td className="px-2 py-2.5">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label={`Actions for ${t.name ?? "tunnel"}`} disabled={!t.editable}>
                            <MoreHorizontal />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent>
                          <DropdownMenuItem onSelect={() => setDialog({ editing: t })}>
                            <Pencil /> Edit
                          </DropdownMenuItem>
                          <DropdownMenuItem destructive onSelect={() => setRemove(t)}>
                            <Trash2 /> Delete
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {dialog && <TunnelDialog servers={servers ?? []} editing={dialog.editing} onClose={() => setDialog(null)} />}
      <ConfirmDialog
        open={remove != null}
        onOpenChange={(o) => !o && setRemove(null)}
        title={`Delete ${remove?.name ?? "this tunnel"}?`}
        description="The tunnel and its public address are removed from your playit.gg account. Players can no longer join through it."
        confirmLabel="Delete tunnel"
        destructive
        onConfirm={async () => {
          if (!remove) return;
          try {
            await api.playit.deleteTunnel(remove.id);
            toast.success("Tunnel deleted");
            await qc.invalidateQueries({ queryKey: qk.playitTunnels });
          } catch (e) {
            toast.error(errorMessage(e));
          }
        }}
      />
    </Card>
  );
}

/** Internet access through playit.gg: MCPanel's agent and the account's tunnels. */
export function PlayitPage() {
  const { data: agent } = usePlayitAgent();
  return (
    <>
      <PageHeader
        title="Playit.gg"
        description="Let friends outside your network join your servers through free playit.gg tunnels, without opening router ports."
        actions={
          <Button variant="outline" onClick={() => open("https://playit.gg/account/tunnels")}>
            playit.gg dashboard <ExternalLink />
          </Button>
        }
      />
      <PageBody className="space-y-5">
        {agent ? <McpanelAgentCard agent={agent} /> : <SkeletonRows rows={2} />}
        {agent?.linked && <TunnelsCard agent={agent} />}
        <PlayitAgentCard />
      </PageBody>
    </>
  );
}
