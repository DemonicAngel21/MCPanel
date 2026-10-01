import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  Activity,
  Check,
  Copy,
  Cpu,
  HardDrive,
  KeyRound,
  Lock,
  MoreHorizontal,
  Network,
  Plus,
  Radio,
  RefreshCw,
  Server,
  Trash2,
  UserCheck,
  Wifi,
  WifiOff,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import type { HostDto } from "@/bindings/HostDto";
import type { HostEnrollmentTokenDto } from "@/bindings/HostEnrollmentTokenDto";
import { Button } from "@/components/ui/button";
import {
  ConfirmDialog,
  Dialog,
  DialogContent,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { Badge, Card, EmptyState, Field, Input, SkeletonRows, Spinner } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes, formatCount, formatRelative } from "@/lib/format";
import { qk, useMultihostStatus } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";

export function HostsPage() {
  const qc = useQueryClient();
  const { data: status, isLoading } = useMultihostStatus();
  const [enrollOpen, setEnrollOpen] = useState(false);
  const [tokenOpen, setTokenOpen] = useState(false);
  const [activeToken, setActiveToken] = useState<HostEnrollmentTokenDto | null>(null);
  const [removingHost, setRemovingHost] = useState<HostDto | null>(null);
  const [pingLatencies, setPingLatencies] = useState<Record<string, number | null>>({});
  const [pinging, setPinging] = useState<Record<string, boolean>>({});

  const refresh = () => qc.invalidateQueries({ queryKey: qk.multihost });

  const pingMutation = useMutation({
    mutationFn: (hostId: string) => api.multihost.ping(hostId),
    onMutate: (hostId) => {
      setPinging((prev) => ({ ...prev, [hostId]: true }));
    },
    onSuccess: (res) => {
      setPinging((prev) => ({ ...prev, [res.hostId]: false }));
      setPingLatencies((prev) => ({ ...prev, [res.hostId]: res.latencyMs }));
      toast.success(res.message ?? `Ping: ${res.latencyMs ?? 0} ms`);
    },
    onError: (err, hostId) => {
      setPinging((prev) => ({ ...prev, [hostId]: false }));
      toast.error(`Ping failed: ${errorMessage(err)}`);
    },
  });

  const removeMutation = useMutation({
    mutationFn: (hostId: string) => api.multihost.remove(hostId),
    onSuccess: () => {
      void refresh();
      setRemovingHost(null);
      toast.success("Host removed from multihost cluster");
    },
    onError: (e) => toast.error(errorMessage(e)),
  });

  const generateToken = async () => {
    try {
      const res = await api.multihost.generateToken();
      setActiveToken(res);
      setTokenOpen(true);
    } catch (e) {
      toast.error(`Could not generate pairing token: ${errorMessage(e)}`);
    }
  };

  const hosts = status?.hosts ?? [];
  const localHost = hosts.find((h) => h.isLocal);
  const remoteHosts = hosts.filter((h) => !h.isLocal);
  const signedIn = status?.signedIn ?? false;
  const totalServers = hosts.reduce((acc, h) => acc + h.serversCount, 0);
  const runningServers = hosts.reduce((acc, h) => acc + h.runningServersCount, 0);

  return (
    <>
      <PageHeader
        title="Hosts"
        description="Manage servers across your local machine and remote nodes. An MCPanel account is required for multihost management."
        actions={
          <>
            <Button variant="outline" size="sm" onClick={refresh} title="Refresh hosts and statuses">
              <RefreshCw className={cn(isLoading && "animate-spin")} /> Refresh
            </Button>
            {signedIn && (
              <>
                <Button variant="outline" size="sm" onClick={generateToken}>
                  <KeyRound /> Pairing token
                </Button>
                <Button variant="primary" size="sm" onClick={() => setEnrollOpen(true)}>
                  <Plus /> Enroll host
                </Button>
              </>
            )}
          </>
        }
      />
      <PageBody className="space-y-6">
        {isLoading ? (
          <Card className="p-4">
            <SkeletonRows rows={4} />
          </Card>
        ) : (
          <>
            {/* Account requirement banner when signed out */}
            {!signedIn && (
              <Card className="relative overflow-hidden border-accent/40 bg-gradient-to-r from-accent/10 via-surface to-surface p-6 shadow-sm">
                <div className="flex flex-col gap-5 sm:flex-row sm:items-center sm:justify-between">
                  <div className="flex items-start gap-4">
                    <div className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-accent/20 text-accent">
                      <Lock className="size-5" />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <h2 className="text-base font-semibold text-fg">MCPanel Account Required for Multihost</h2>
                        <Badge tone="warning">Account Needed</Badge>
                      </div>
                      <p className="mt-1.5 max-w-xl text-xs leading-relaxed text-muted">
                        Multihost clustering allows you to enroll remote nodes running{" "}
                        <code className="rounded bg-surface-2 px-1 py-0.5 font-mono text-[11px] text-fg">mcpanel-node</code>, monitor latency, and
                        operate servers across physical machines in one unified view.
                      </p>
                      <div className="mt-3 flex flex-wrap gap-2 text-xs text-muted">
                        <span className="flex items-center gap-1 rounded-md bg-surface-2 px-2 py-1">
                          <Network className="size-3.5 text-accent" /> Remote Node Management
                        </span>
                        <span className="flex items-center gap-1 rounded-md bg-surface-2 px-2 py-1">
                          <Radio className="text-success size-3.5" /> Live Latency Pings
                        </span>
                        <span className="flex items-center gap-1 rounded-md bg-surface-2 px-2 py-1">
                          <UserCheck className="size-3.5 text-accent" /> Secure Token Pairing
                        </span>
                      </div>
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <Button asChild variant="primary">
                      <Link to="/account">Sign In or Create Account</Link>
                    </Button>
                  </div>
                </div>
              </Card>
            )}

            {/* Authenticated summary stats */}
            {signedIn && (
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                <Card className="p-3.5">
                  <div className="flex items-center justify-between text-xs text-muted">
                    <span>Total Nodes</span>
                    <Network className="size-4 text-accent" />
                  </div>
                  <div className="mt-1 text-2xl font-bold text-fg">{hosts.length}</div>
                  <div className="mt-0.5 text-[11px] text-faint">1 local · {remoteHosts.length} remote</div>
                </Card>
                <Card className="p-3.5">
                  <div className="flex items-center justify-between text-xs text-muted">
                    <span>Account</span>
                    <UserCheck className="text-success size-4" />
                  </div>
                  <div className="mt-1 truncate text-sm font-semibold text-fg" title={status?.userEmail ?? ""}>
                    {status?.userEmail ?? "Connected"}
                  </div>
                  <div className="text-success mt-0.5 text-[11px]">Cluster active</div>
                </Card>
                <Card className="p-3.5">
                  <div className="flex items-center justify-between text-xs text-muted">
                    <span>Servers Hosted</span>
                    <Server className="size-4 text-accent" />
                  </div>
                  <div className="mt-1 text-2xl font-bold text-fg">{totalServers}</div>
                  <div className="mt-0.5 text-[11px] text-faint">Across all enrolled nodes</div>
                </Card>
                <Card className="p-3.5">
                  <div className="flex items-center justify-between text-xs text-muted">
                    <span>Active Processes</span>
                    <Activity className="text-success size-4" />
                  </div>
                  <div className="mt-1 text-2xl font-bold text-fg">{runningServers}</div>
                  <div className="mt-0.5 text-[11px] text-faint">{formatCount(runningServers, "server")} running</div>
                </Card>
              </div>
            )}

            {/* Local Host Node Card */}
            <div>
              <div className="mb-2.5 flex items-center justify-between">
                <h3 className="text-xs font-semibold tracking-wider text-muted uppercase">Primary Machine</h3>
                <Badge tone="neutral">Local Host</Badge>
              </div>
              {localHost && (
                <Card className="border-border-strong p-4 transition-colors hover:border-accent/40">
                  <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-fg">{localHost.name}</span>
                        <Badge tone="success">
                          <span className="bg-success mr-1 inline-block size-1.5 animate-pulse rounded-full" />
                          Online
                        </Badge>
                        <Badge tone="neutral">Loopback</Badge>
                      </div>
                      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted">
                        {localHost.osInfo && <span>{localHost.osInfo}</span>}
                        {localHost.cpuCount != null && (
                          <span className="flex items-center gap-1">
                            <Cpu className="size-3 text-faint" /> {localHost.cpuCount} CPU cores
                          </span>
                        )}
                        {localHost.totalMemoryBytes != null && (
                          <span className="flex items-center gap-1">
                            <HardDrive className="size-3 text-faint" />
                            {formatBytes(localHost.usedMemoryBytes)} / {formatBytes(localHost.totalMemoryBytes)} RAM
                          </span>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-4 text-xs">
                      <div className="rounded-lg bg-surface-2 px-3 py-1.5 text-center">
                        <div className="font-semibold text-fg">
                          {localHost.runningServersCount} / {localHost.serversCount}
                        </div>
                        <div className="text-[11px] text-muted">Running Servers</div>
                      </div>
                      <div className="rounded-lg bg-surface-2 px-3 py-1.5 text-center">
                        <div className="text-success font-semibold">0 ms</div>
                        <div className="text-[11px] text-muted">Latency</div>
                      </div>
                    </div>
                  </div>
                </Card>
              )}
            </div>

            {/* Remote Hosts Section */}
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-xs font-semibold tracking-wider text-muted uppercase">Enrolled Remote Nodes</h3>
                  <p className="text-[11px] text-faint">Nodes running MCPanel daemon or remote agent linked to your account.</p>
                </div>
                {signedIn && remoteHosts.length > 0 && (
                  <Button variant="outline" size="sm" onClick={() => setEnrollOpen(true)}>
                    <Plus /> Add Node
                  </Button>
                )}
              </div>

              {!signedIn ? (
                <Card className="p-8 text-center">
                  <Network className="mx-auto size-9 text-muted/60" />
                  <h4 className="mt-3 text-sm font-medium text-fg">Remote Nodes Locked</h4>
                  <p className="mx-auto mt-1 max-w-md text-xs text-muted">
                    Sign in to your MCPanel account to unlock remote node management and add dedicated servers or VPS instances.
                  </p>
                  <Button asChild variant="outline" size="sm" className="mt-4">
                    <Link to="/account">Sign In to Continue</Link>
                  </Button>
                </Card>
              ) : remoteHosts.length === 0 ? (
                <EmptyState
                  icon={<Network />}
                  title="No remote nodes enrolled yet"
                  description="Add remote VPS, cloud, or dedicated servers to monitor hardware and manage Minecraft instances alongside your local machine."
                  action={
                    <div className="flex items-center gap-2">
                      <Button variant="outline" size="sm" onClick={generateToken}>
                        <KeyRound /> Pair Node with Token
                      </Button>
                      <Button variant="primary" size="sm" onClick={() => setEnrollOpen(true)}>
                        <Plus /> Enroll Host Manually
                      </Button>
                    </div>
                  }
                />
              ) : (
                <div className="grid gap-3 sm:grid-cols-2">
                  {remoteHosts.map((host) => {
                    const latency = pingLatencies[host.id] ?? host.latencyMs;
                    const isPinging = pinging[host.id] ?? false;

                    return (
                      <Card key={host.id} className="flex flex-col justify-between p-4">
                        <div className="space-y-2.5">
                          <div className="flex items-start justify-between gap-2">
                            <div>
                              <div className="flex items-center gap-2">
                                <span className="font-semibold text-fg">{host.name}</span>
                                <Badge tone={host.status === "online" ? "success" : host.status === "offline" ? "neutral" : "warning"}>
                                  {host.status === "online" ? <Wifi className="size-3" /> : <WifiOff className="size-3" />}
                                  {host.status}
                                </Badge>
                              </div>
                              {host.endpoint && (
                                <p className="mt-1 max-w-xs truncate font-mono text-[11px] text-muted" title={host.endpoint}>
                                  {host.endpoint}
                                </p>
                              )}
                            </div>

                            <DropdownMenu>
                              <DropdownMenuTrigger asChild>
                                <Button variant="ghost" size="icon-sm" aria-label="Host options">
                                  <MoreHorizontal />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                <DropdownMenuItem onClick={() => pingMutation.mutate(host.id)}>
                                  <Radio /> Ping Node
                                </DropdownMenuItem>
                                {host.endpoint && (
                                  <DropdownMenuItem
                                    onClick={() => {
                                      void navigator.clipboard.writeText(host.endpoint ?? "");
                                      toast.success("Endpoint copied to clipboard");
                                    }}
                                  >
                                    <Copy /> Copy Endpoint
                                  </DropdownMenuItem>
                                )}
                                <DropdownMenuItem destructive onClick={() => setRemovingHost(host)}>
                                  <Trash2 /> Remove Host
                                </DropdownMenuItem>
                              </DropdownMenuContent>
                            </DropdownMenu>
                          </div>

                          <div className="flex flex-wrap gap-1">
                            {host.tags.map((t) => (
                              <span key={t} className="rounded bg-surface-2 px-1.5 py-0.5 text-[10px] text-muted">
                                #{t}
                              </span>
                            ))}
                          </div>
                        </div>

                        <div className="mt-4 flex items-center justify-between border-t border-border pt-3 text-xs text-muted">
                          <div className="flex items-center gap-3">
                            <span title="Running servers on this host">
                              <span className="font-semibold text-fg">{host.runningServersCount}</span>
                              <span className="text-[11px] text-faint">/{host.serversCount} running</span>
                            </span>
                            <span>·</span>
                            <span className="text-[11px] text-faint">seen {formatRelative(host.lastSeen)}</span>
                          </div>

                          <div className="flex items-center gap-2">
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-7 px-2 text-xs"
                              disabled={isPinging}
                              onClick={() => pingMutation.mutate(host.id)}
                            >
                              {isPinging ? (
                                <Spinner className="size-3 text-accent" />
                              ) : (
                                <span className={cn(latency && latency < 50 ? "text-success" : "text-fg")}>
                                  {latency != null ? `${latency} ms` : "Ping"}
                                </span>
                              )}
                            </Button>
                          </div>
                        </div>
                      </Card>
                    );
                  })}
                </div>
              )}
            </div>
          </>
        )}
      </PageBody>

      {/* Enroll Host Dialog */}
      <EnrollHostDialog open={enrollOpen} onOpenChange={setEnrollOpen} onEnrolled={refresh} />

      {/* Pairing Token Dialog */}
      {activeToken && <PairingTokenDialog open={tokenOpen} onOpenChange={setTokenOpen} token={activeToken} />}

      {/* Remove Host Confirmation */}
      {removingHost && (
        <ConfirmDialog
          open={Boolean(removingHost)}
          onOpenChange={(open) => !open && setRemovingHost(null)}
          title={`Remove '${removingHost.name}'?`}
          description="Removing this host disconnects it from your MCPanel cluster. Servers running on the remote node will not be deleted."
          confirmLabel="Remove Host"
          destructive
          onConfirm={() => removeMutation.mutate(removingHost.id)}
        />
      )}
    </>
  );
}

function EnrollHostDialog({ open, onOpenChange, onEnrolled }: { open: boolean; onOpenChange: (open: boolean) => void; onEnrolled: () => void }) {
  const [name, setName] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [authToken, setAuthToken] = useState("");
  const [tags, setTags] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const reset = () => {
    setName("");
    setEndpoint("");
    setAuthToken("");
    setTags("");
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      toast.error("Host name is required");
      return;
    }
    if (!endpoint.trim()) {
      toast.error("Endpoint URL is required");
      return;
    }

    setSubmitting(true);
    try {
      const cleanTags = tags
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);

      await api.multihost.add({
        name: name.trim(),
        endpoint: endpoint.trim(),
        authToken: authToken.trim() || null,
        tags: cleanTags.length > 0 ? cleanTags : ["remote"],
      });

      toast.success(`Host '${name}' enrolled successfully`);
      reset();
      onOpenChange(false);
      onEnrolled();
    } catch (err) {
      toast.error(errorMessage(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        title="Enroll Remote Host"
        description="Connect a remote server running MCPanel Node or compatible daemon."
        footer={
          <>
            <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={submitting}>
              Cancel
            </Button>
            <Button variant="primary" onClick={submit} disabled={submitting}>
              {submitting ? <Spinner /> : <Plus />} Enroll Host
            </Button>
          </>
        }
      >
        <form onSubmit={submit} className="space-y-4">
          <Field label="Host Name" hint="A friendly name to identify this machine.">
            <Input placeholder="e.g. Frankfurt VPS 1" value={name} onChange={(e) => setName(e.target.value)} disabled={submitting} autoFocus />
          </Field>

          <Field label="Endpoint Address" hint="The HTTPS or HTTP URL where the node agent listens.">
            <Input
              placeholder="https://node1.example.com:8443"
              value={endpoint}
              onChange={(e) => setEndpoint(e.target.value)}
              disabled={submitting}
            />
          </Field>

          <Field label="Node Secret Token (Optional)" hint="Authentication secret configured on the remote node.">
            <Input
              type="password"
              placeholder="Secret token"
              value={authToken}
              onChange={(e) => setAuthToken(e.target.value)}
              disabled={submitting}
            />
          </Field>

          <Field label="Tags (Optional)" hint="Comma-separated labels for grouping (e.g. europe, survival, vps).">
            <Input placeholder="vps, europe, dedicated" value={tags} onChange={(e) => setTags(e.target.value)} disabled={submitting} />
          </Field>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function PairingTokenDialog({ open, onOpenChange, token }: { open: boolean; onOpenChange: (open: boolean) => void; token: HostEnrollmentTokenDto }) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(token.pairingCommand);
      setCopied(true);
      toast.success("Pairing command copied to clipboard");
      setTimeout(() => setCopied(false), 2000);
    } catch {
      toast.error("Failed to copy command");
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        title="Node Pairing Command"
        description="Run this command on your remote server to enroll it into your MCPanel cluster."
        footer={
          <Button variant="primary" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        }
      >
        <div className="space-y-4">
          <p className="text-xs text-muted">
            The enrollment token is valid for 1 hour. It securely associates the remote node with your account UID{" "}
            <code className="rounded bg-surface-2 px-1 py-0.5 font-mono text-[11px] text-fg">{token.accountUid}</code>.
          </p>

          <div className="relative rounded-lg border border-border bg-surface-2 p-3 font-mono text-xs text-fg">
            <pre className="selectable pr-8 break-all whitespace-pre-wrap">{token.pairingCommand}</pre>
            <Button variant="ghost" size="icon-sm" className="absolute top-2 right-2 text-muted hover:text-fg" onClick={copy} title="Copy command">
              {copied ? <Check className="text-success size-4" /> : <Copy className="size-4" />}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
