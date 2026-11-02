import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Activity, Cloud, Copy, Cpu, HardDrive, MoreHorizontal, Network, Plus, Power, RefreshCw, Server, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import type { HostDto } from "@/bindings/HostDto";
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
import { Badge, Card, EmptyState, Field, Input, SkeletonRows, Spinner, StatusDot } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes, formatCount } from "@/lib/format";
import { qk, useMultihostStatus } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";

export function HostsPage() {
  const qc = useQueryClient();
  const { data: status, isLoading } = useMultihostStatus();
  const googleAuth = useQuery({
    queryKey: ["google-compute-auth"],
    queryFn: api.account.googleComputeStatus,
    refetchInterval: (query) => query.state.data?.state === "waiting" ? 1000 : false,
  });
  const [enrollOpen, setEnrollOpen] = useState(false);
  const [removingHost, setRemovingHost] = useState<HostDto | null>(null);
  const [powerAction, setPowerAction] = useState<{ host: HostDto; start: boolean } | null>(null);

  const refresh = () => qc.invalidateQueries({ queryKey: qk.multihost });

  const removeMutation = useMutation({
    mutationFn: (hostId: string) => api.multihost.remove(hostId),
    onSuccess: () => {
      void refresh();
      setRemovingHost(null);
      toast.success("Saved host removed");
    },
    onError: (e) => toast.error(errorMessage(e)),
  });

  const refreshGoogleMutation = useMutation({
    mutationFn: (hostId: string) => api.multihost.refreshGoogle(hostId),
    onSuccess: (host) => {
      void refresh();
      toast.success(`${host.name} VM details refreshed`);
    },
    onError: (error) => toast.error(`Google Cloud connection failed: ${errorMessage(error)}`),
  });

  const connectGoogleMutation = useMutation({
    mutationFn: api.account.connectGoogleCompute,
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["google-compute-auth"] }),
    onError: (error) => toast.error(`Google Cloud authorization failed: ${errorMessage(error)}`),
  });

  useEffect(() => {
    if (googleAuth.data?.state === "done") {
      toast.success("Google Cloud connected");
      void qc.invalidateQueries({ queryKey: qk.multihost });
    } else if (googleAuth.data?.state === "failed") {
      toast.error(googleAuth.data.message ?? "Google Cloud authorization failed");
    }
  }, [googleAuth.data?.state, googleAuth.data?.message, qc]);

  const powerGoogleMutation = useMutation({
    mutationFn: ({ hostId, start }: { hostId: string; start: boolean }) => api.multihost.setGooglePower(hostId, start),
    onSuccess: (_, variables) => {
      void refresh();
      setPowerAction(null);
      toast.success(variables.start ? "Google Cloud VM start requested" : "Google Cloud VM stop requested");
    },
    onError: (error) => toast.error(`Google Cloud action failed: ${errorMessage(error)}`),
  });

  const hosts = status?.hosts ?? [];
  const localHost = hosts.find((h) => h.isLocal);
  const remoteHosts = hosts.filter((h) => !h.isLocal && h.googleCloud != null);
  const totalServers = hosts.reduce((acc, h) => acc + h.serversCount, 0);
  const runningServers = hosts.reduce((acc, h) => acc + h.runningServersCount, 0);

  return (
    <>
      <PageHeader
        title="Hosts"
        description="Connect and control Google Compute Engine VMs directly from MCPanel."
        actions={
          <>
            <Button variant="outline" size="sm" onClick={refresh} title="Refresh hosts and statuses">
              <RefreshCw className={cn(isLoading && "animate-spin")} /> Refresh
            </Button>
            {!googleAuth.data?.connected && (
              <Button variant="primary" size="sm" onClick={() => connectGoogleMutation.mutate()} disabled={connectGoogleMutation.isPending}>
                {connectGoogleMutation.isPending ? <Spinner /> : <Cloud />} Connect Google Cloud
              </Button>
            )}
            <Button variant="primary" size="sm" onClick={() => setEnrollOpen(true)} disabled={!googleAuth.data?.connected}>
              <Plus /> Add Google VM
            </Button>
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
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <Card className="p-3.5">
                <div className="flex items-center justify-between text-xs text-muted">
                  <span>Hosts</span>
                  <Network className="size-4 text-accent" />
                </div>
                <div className="mt-1 text-2xl font-bold text-fg">{hosts.length}</div>
                <div className="mt-0.5 text-[11px] text-faint">{remoteHosts.length} Google Cloud VM(s)</div>
              </Card>
              <Card className="p-3.5">
                <div className="flex items-center justify-between text-xs text-muted">
                  <span>Google Cloud</span>
                  <Cloud className="size-4 text-accent" />
                </div>
                <div className="mt-1 truncate text-sm font-semibold text-fg">Compute Engine API</div>
                <div className="text-success mt-0.5 text-[11px]">{googleAuth.data?.connected ? "OAuth connected" : "Authorization required"}</div>
              </Card>
              <Card className="p-3.5">
                <div className="flex items-center justify-between text-xs text-muted">
                  <span>Servers Hosted</span>
                  <Server className="size-4 text-accent" />
                </div>
                <div className="mt-1 text-2xl font-bold text-fg">{totalServers}</div>
                <div className="mt-0.5 text-[11px] text-faint">Reported by available hosts</div>
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
                          <StatusDot tone="success" pulse />
                          <span>Online</span>
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
                    </div>
                  </div>
                </Card>
              )}
            </div>

            {/* Remote Hosts Section */}
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-xs font-semibold tracking-wider text-muted uppercase">Google Compute Engine</h3>
                  <p className="text-[11px] text-faint">Live VM state and network details come from the Compute Engine API.</p>
                </div>
                {remoteHosts.length > 0 && (
                  <Button variant="outline" size="sm" onClick={() => setEnrollOpen(true)} disabled={!googleAuth.data?.connected}>
                    <Plus /> Add Google VM
                  </Button>
                )}
              </div>

              {remoteHosts.length === 0 ? (
                <EmptyState
                  icon={<Cloud />}
                  title="No Google Cloud VMs connected"
                  description="Connect Google Cloud, then add an existing VM by project, zone, and instance name."
                  action={
                    <Button variant="primary" size="sm" onClick={() => setEnrollOpen(true)}>
                      <Plus /> Add Google VM
                    </Button>
                  }
                />
              ) : (
                <div className="grid gap-3 sm:grid-cols-2">
                  {remoteHosts.map((host) => {
                    const vm = host.googleCloud;
                    if (!vm) return null;
                    const isOnline = host.status === "online";
                    const isOffline = host.status === "offline";
                    const isStarting = host.status === "starting";
                    const isStopping = host.status === "stopping";
                    return (
                      <Card key={host.id} className="flex flex-col justify-between p-4">
                        <div className="space-y-2.5">
                          <div className="flex items-start justify-between gap-2">
                            <div>
                              <div className="flex items-center gap-2">
                                <span className="font-semibold text-fg">{host.name}</span>
                                <Badge tone={isOnline ? "success" : isOffline ? "neutral" : "warning"}>
                                  <StatusDot tone={isOnline ? "success" : "neutral"} pulse={isOnline} />
                                  <span>{isOnline ? "VM running" : isOffline ? "VM stopped" : isStarting ? "Starting" : isStopping ? "Stopping" : "VM unavailable"}</span>
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

                          <div className="text-xs text-muted">
                            <div className="font-medium text-fg">{vm.instanceName}</div>
                            <div>
                              {vm.projectId} · {vm.zone}
                            </div>
                          </div>

                          {(host.osInfo || host.cpuCount != null || host.totalMemoryBytes != null) && (
                            <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted">
                              {host.osInfo && <span>{host.osInfo}</span>}
                              {host.cpuCount != null && <span>{host.cpuCount} vCPU</span>}
                              {host.totalMemoryBytes != null && (
                                <span>
                                  {formatBytes(host.usedMemoryBytes)} / {formatBytes(host.totalMemoryBytes)} RAM
                                </span>
                              )}
                            </div>
                          )}

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
                            <span className="text-[11px] text-faint">
                              {host.lastSeen ? `Checked ${new Date(host.lastSeen).toLocaleString()}` : "VM details saved locally"}
                            </span>
                          </div>
                          <Button
                            variant="outline"
                            size="sm"
                            disabled={refreshGoogleMutation.isPending}
                            onClick={() => refreshGoogleMutation.mutate(host.id)}
                          >
                            {refreshGoogleMutation.isPending ? <Spinner /> : <RefreshCw />}
                            Refresh VM
                          </Button>
                          <Button
                            variant={isOnline ? "outline" : "primary"}
                            size="sm"
                            disabled={powerGoogleMutation.isPending || (!isOnline && !isOffline)}
                            onClick={() => setPowerAction({ host, start: isOffline })}
                          >
                            <Power /> {isOnline ? "Stop VM" : isStarting ? "Starting" : isStopping ? "Stopping" : "Start VM"}
                          </Button>
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

      {/* Add Google Compute Engine VM */}
      <EnrollHostDialog open={enrollOpen} onOpenChange={setEnrollOpen} onEnrolled={refresh} />

      {powerAction && (
        <ConfirmDialog
          open={Boolean(powerAction)}
          onOpenChange={(open) => !open && setPowerAction(null)}
          title={`${powerAction.start ? "Start" : "Stop"} '${powerAction.host.name}'?`}
          description={powerAction.start
            ? "This will start the Google Cloud VM. Compute Engine charges may apply while it runs."
            : "This will stop the VM and disconnect any players or processes running on it."}
          confirmLabel={powerAction.start ? "Start VM" : "Stop VM"}
          destructive={!powerAction.start}
          onConfirm={() => powerGoogleMutation.mutate({ hostId: powerAction.host.id, start: powerAction.start })}
        />
      )}

      {/* Remove Host Confirmation */}
      {removingHost && (
        <ConfirmDialog
          open={Boolean(removingHost)}
          onOpenChange={(open) => !open && setRemovingHost(null)}
          title={`Remove '${removingHost.name}'?`}
          description="Remove this VM reference from MCPanel. This does not stop or delete the Google Cloud VM."
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
  const [projectId, setProjectId] = useState("");
  const [zone, setZone] = useState("");
  const [instanceName, setInstanceName] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const reset = () => {
    setName("");
    setProjectId("");
    setZone("");
    setInstanceName("");
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      toast.error("Host name is required");
      return;
    }
    if (!projectId.trim() || !zone.trim() || !instanceName.trim()) {
      toast.error("Project ID, zone, and instance name are required.");
      return;
    }

    setSubmitting(true);
    try {
      await api.multihost.addGoogle({
        name: name.trim(),
        projectId: projectId.trim(),
        zone: zone.trim(),
        instanceName: instanceName.trim(),
      });

      toast.success(`Google Cloud VM '${name}' saved.`);
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
        title="Add Google Compute Engine VM"
        description="Connect an existing VM with the Google account authorized for Compute Engine access."
        footer={
          <>
            <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={submitting}>
              Cancel
            </Button>
            <Button variant="primary" onClick={submit} disabled={submitting}>
              {submitting ? <Spinner /> : <Plus />} Add VM
            </Button>
          </>
        }
      >
        <form onSubmit={submit} className="space-y-4">
          <Field label="Display Name">
            <Input placeholder="e.g. Minecraft Europe" value={name} onChange={(e) => setName(e.target.value)} disabled={submitting} autoFocus />
          </Field>

          <Field label="Google Cloud Project ID">
            <Input placeholder="my-minecraft-project" value={projectId} onChange={(e) => setProjectId(e.target.value)} disabled={submitting} />
          </Field>

          <Field label="Zone" hint="For example, us-central1-a">
            <Input placeholder="us-central1-a" value={zone} onChange={(e) => setZone(e.target.value)} disabled={submitting} />
          </Field>

          <Field label="VM Instance Name">
            <Input placeholder="minecraft-server-1" value={instanceName} onChange={(e) => setInstanceName(e.target.value)} disabled={submitting} />
          </Field>

          <p className="text-xs text-muted">
            Connect Google Cloud first. MCPanel uses your Google OAuth authorization and does not require the Google Cloud CLI or inbound SSH access.
          </p>
        </form>
      </DialogContent>
    </Dialog>
  );
}
