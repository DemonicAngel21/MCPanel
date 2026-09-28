import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowUpCircle,
  Download,
  ExternalLink,
  MoreHorizontal,
  PackagePlus,
  Power,
  PowerOff,
  RefreshCw,
  RotateCcw,
  Search,
  Trash2,
  Undo2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import type { ContentEntryDto } from "@/bindings/ContentEntryDto";
import type { ContentListDto } from "@/bindings/ContentListDto";
import type { ProjectDto } from "@/bindings/ProjectDto";
import type { UpdateInfoDto } from "@/bindings/UpdateInfoDto";
import { PageBody } from "@/app/app-shell";
import { Button } from "@/components/ui/button";
import {
  ConfirmDialog,
  Dialog,
  DialogClose,
  DialogContent,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Select,
} from "@/components/ui/overlays";
import { Badge, Banner, Card, CardHeader, Checkbox, EmptyState, Input, Spinner, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes, formatRelative } from "@/lib/format";
import { waitForJob } from "@/lib/jobs";
import { qk, useContent, useServer } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

const PENDING_LABEL: Record<string, string> = {
  install: "Installs on next restart",
  remove: "Removed on next restart",
  disable: "Disabled on next restart",
  enable: "Enabled on next restart",
};

function openPage(url: string) {
  api.app.openExternal(url).catch((e) => toast.error(errorMessage(e)));
}

function compact(n: number) {
  return new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

/** Plan preview + confirm for installing a project (with required dependencies). */
function InstallDialog({
  serverId,
  project,
  versionId,
  onClose,
}: {
  serverId: string;
  project: { provider: string; id: string; name: string };
  versionId: string | null;
  onClose: () => void;
}) {
  const [withDeps, setWithDeps] = useState(true);
  const [version, setVersion] = useState<string | null>(versionId);
  const [busy, setBusy] = useState(false);
  const versions = useQuery({
    queryKey: ["content-versions", serverId, project.provider, project.id],
    queryFn: () => api.content.versions(serverId, project.provider, project.id),
    retry: false,
  });
  const request = { provider: project.provider, projectId: project.id, versionId: version, withDependencies: withDeps };
  const plan = useQuery({
    queryKey: ["content-plan", serverId, request],
    queryFn: () => api.content.plan(serverId, request),
    retry: false,
    gcTime: 0,
  });

  const install = async () => {
    setBusy(true);
    try {
      const job = await waitForJob(await api.content.install(serverId, request));
      if (job.status === "succeeded") {
        const deferred = (job.result as { deferred?: boolean } | null)?.deferred;
        toast.success(deferred ? `${project.name} will be installed when the server stops or restarts` : `${project.name} installed`);
        onClose();
      }
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={`Install ${project.name}`}
        description="Files are downloaded over HTTPS, checked against the published hash and validated as plugins before they are placed."
        className="w-[min(600px,calc(100vw-32px))]"
        footer={
          <>
            <DialogClose asChild>
              <Button variant="ghost">Cancel</Button>
            </DialogClose>
            <Button variant="primary" disabled={busy || !plan.data || plan.data.items.length === 0} onClick={install}>
              {busy ? <Spinner className="text-accent-fg" /> : <Download />} Install
              {plan.data && plan.data.items.length > 1 ? ` ${plan.data.items.length} plugins` : ""}
            </Button>
          </>
        }
      >
        <div className="space-y-3 text-xs">
          <div className="flex items-center gap-3">
            <Select
              className="w-64"
              value={version ?? "latest"}
              onValueChange={(v) => setVersion(v === "latest" ? null : v)}
              options={[
                { value: "latest", label: "Newest compatible version" },
                ...(versions.data ?? []).map((v) => ({ value: v.id, label: v.versionNumber, hint: v.channel === "release" ? undefined : v.channel })),
              ]}
            />
            <label className="flex items-center gap-2 text-fg">
              <Checkbox checked={withDeps} onCheckedChange={(c) => setWithDeps(c === true)} /> Install required dependencies
            </label>
          </div>
          {plan.isLoading && <Spinner />}
          {plan.error && <p className="text-danger">{errorMessage(plan.error)}</p>}
          {plan.data && (
            <>
              {plan.data.deferred && (
                <Banner tone="info" title="The server is running">
                  The files are downloaded and verified now, and installed when the server stops or restarts.
                </Banner>
              )}
              <ul className="divide-y divide-border rounded-md border border-border">
                {plan.data.items.map((i) => (
                  <li key={`${i.project.id}`} className="flex items-center justify-between gap-3 px-3 py-2">
                    <div className="min-w-0">
                      <p className="truncate text-[13px] text-fg">
                        {i.project.name} <span className="text-muted">{i.version.versionNumber}</span>
                      </p>
                      <p className="truncate text-muted">
                        {i.requiredBy ? `Required by ${i.requiredBy}` : "Requested"}
                        {i.replaces && ` · replaces ${i.replaces}`}
                        {i.version.sizeBytes != null && ` · ${formatBytes(i.version.sizeBytes)}`}
                      </p>
                    </div>
                    {i.version.hashAlgorithm ? (
                      <Badge tone="success">{i.version.hashAlgorithm.toUpperCase()} verified</Badge>
                    ) : (
                      <Badge tone="warning">Unverified</Badge>
                    )}
                  </li>
                ))}
              </ul>
              {plan.data.unresolved.map((u) => (
                <p key={u} className="text-warning">
                  Needs manual install: {u}
                </p>
              ))}
              {plan.data.warnings.map((w) => (
                <p key={w} className="text-warning">
                  {w}
                </p>
              ))}
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Browser({ serverId, list, onInstall }: { serverId: string; list: ContentListDto; onInstall: (p: ProjectDto) => void }) {
  const [provider, setProvider] = useState(list.providers[0]?.id ?? "");
  const [text, setText] = useState("");
  const [sort, setSort] = useState("relevance");
  const query = useDebounced(text, 350);
  const results = useQuery({
    queryKey: ["content-search", serverId, provider, query, sort],
    queryFn: () => api.content.search(serverId, { provider, text: query, sort, offset: 0, limit: 20 }),
    enabled: !!provider,
    staleTime: 60_000,
  });
  const installed = new Set(list.entries.map((e) => `${e.provider}:${e.projectId}`));
  return (
    <Card>
      <div className="flex flex-wrap items-center gap-2 border-b border-border p-3">
        <div className="flex rounded-md border border-border-strong p-0.5">
          {list.providers.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => setProvider(p.id)}
              className={cn("rounded px-3 py-1 text-[13px]", provider === p.id ? "bg-surface-3 font-medium text-fg" : "text-muted hover:text-fg")}
            >
              {p.displayName}
            </button>
          ))}
        </div>
        <div className="relative min-w-56 flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-faint" />
          <Input value={text} onChange={(e) => setText(e.target.value)} placeholder={`Search ${list.kind}s…`} className="pl-8" aria-label="Search" />
        </div>
        <Select
          className="w-40"
          value={sort}
          onValueChange={setSort}
          options={[
            { value: "relevance", label: "Relevance" },
            { value: "downloads", label: "Most downloads" },
            { value: "updated", label: "Recently updated" },
          ]}
        />
      </div>
      {results.isLoading && <Spinner className="m-6" />}
      {results.error && <p className="p-4 text-xs text-danger">{errorMessage(results.error)}</p>}
      {results.data && results.data.hits.length === 0 && <EmptyState title="No results" description="Try another search or provider." />}
      <ul className="divide-y divide-border">
        {results.data?.hits.map((p) => (
          <li key={p.id} className="flex items-start gap-3 px-4 py-3 hover:bg-surface-2">
            {p.iconUrl ? (
              <img src={p.iconUrl} alt="" className="size-10 shrink-0 rounded-md bg-surface-3" loading="lazy" />
            ) : (
              <div className="size-10 shrink-0 rounded-md bg-surface-3" />
            )}
            <div className="min-w-0 flex-1">
              <p className="truncate text-[13px] font-medium text-fg">
                {p.name} {p.author && <span className="font-normal text-muted">by {p.author}</span>}
              </p>
              <p className="line-clamp-2 text-xs text-muted">{p.description}</p>
              <p className="mt-0.5 text-[11px] text-faint">
                {compact(p.downloads)} downloads{p.updated && ` · updated ${formatRelative(p.updated)}`}
                {p.license && ` · ${p.license}`}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-1">
              <Button variant="ghost" size="icon-sm" aria-label="Open project page" title="Open project page" onClick={() => openPage(p.pageUrl)}>
                <ExternalLink />
              </Button>
              <Button size="sm" disabled={installed.has(`${p.provider}:${p.id}`)} onClick={() => onInstall(p)}>
                {installed.has(`${p.provider}:${p.id}`) ? "Installed" : "Install…"}
              </Button>
            </div>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function EntryRow({
  e,
  list,
  update,
  serverId,
  onUpdate,
  onRemove,
}: {
  e: ContentEntryDto;
  list: ContentListDto;
  update?: UpdateInfoDto;
  serverId: string;
  onUpdate: () => void;
  onRemove: () => void;
}) {
  const provider = list.providers.find((p) => p.id === e.provider);
  const toggle = async () => {
    try {
      const deferred = await api.content.setEnabled(serverId, e.fileName, !e.enabled);
      toast.success(
        deferred ? `${e.name} will be ${e.enabled ? "disabled" : "enabled"} on the next restart` : `${e.name} ${e.enabled ? "disabled" : "enabled"}`,
      );
    } catch (err) {
      toast.error(errorMessage(err));
    }
  };
  return (
    <tr className={cn("border-b border-border last:border-0 hover:bg-surface-2", !e.enabled && "opacity-60")}>
      <td className="px-4 py-2">
        <p className="text-fg">{e.name}</p>
        <p className="font-mono text-[11px] text-faint">{e.fileName}</p>
      </td>
      <td className="px-4 py-2 text-muted">{e.version ?? "—"}</td>
      <td className="px-4 py-2">
        <div className="flex flex-wrap gap-1">
          {provider ? (
            <Badge tone="info">{provider.displayName}</Badge>
          ) : (
            <Tooltip content="Not installed by MCPanel. “Check for updates” identifies files from Modrinth by their hash.">
              <Badge>Manual</Badge>
            </Tooltip>
          )}
          {!e.enabled && <Badge>Disabled</Badge>}
          {!e.descriptorFormat && (
            <Tooltip content="No plugin.yml / mod descriptor found in this jar">
              <Badge tone="warning">Unknown</Badge>
            </Tooltip>
          )}
          {e.pending && <Badge tone="warning">{PENDING_LABEL[e.pending] ?? e.pending}</Badge>}
          {update && <Badge tone="success">Update: {update.latest.versionNumber}</Badge>}
        </div>
      </td>
      <td className="px-4 py-2 text-muted">{formatBytes(e.sizeBytes)}</td>
      <td className="px-2 py-1 text-right">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" aria-label={`Actions for ${e.name}`}>
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            {update && (
              <DropdownMenuItem onSelect={onUpdate}>
                <ArrowUpCircle /> Update to {update.latest.versionNumber}…
              </DropdownMenuItem>
            )}
            <DropdownMenuItem onSelect={() => void toggle()}>
              {e.enabled ? (
                <>
                  <PowerOff /> Disable
                </>
              ) : (
                <>
                  <Power /> Enable
                </>
              )}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem destructive onSelect={onRemove}>
              <Trash2 /> Remove…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </td>
    </tr>
  );
}

export function ServerContent() {
  const id = useServerId();
  const qc = useQueryClient();
  const { data: server } = useServer(id);
  const { data: list, isLoading, error } = useContent(id);
  const [browse, setBrowse] = useState(false);
  const [installing, setInstalling] = useState<{ provider: string; id: string; name: string; version: string | null } | null>(null);
  const [removing, setRemoving] = useState<ContentEntryDto | null>(null);
  const [updates, setUpdates] = useState<UpdateInfoDto[] | null>(null);
  const [checking, setChecking] = useState(false);

  if (isLoading) return <Spinner className="m-6" />;
  if (!list) return <EmptyState title="Plugins are unavailable" description={error ? errorMessage(error) : undefined} />;
  const label = list.kind === "mod" ? "Mods" : "Plugins";

  const check = async () => {
    setChecking(true);
    try {
      const u = await api.content.checkUpdates(id);
      setUpdates(u);
      toast.success(u.length ? `${u.length} update${u.length === 1 ? "" : "s"} available` : "Everything is up to date");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setChecking(false);
    }
  };

  return (
    <PageBody className="max-w-5xl space-y-4">
      {list.running && list.pending.length > 0 && (
        <Banner
          tone="info"
          icon={<RefreshCw />}
          title={`${list.pending.length} change${list.pending.length === 1 ? "" : "s"} waiting for a restart`}
          actions={
            <Button size="sm" variant="primary" onClick={() => api.servers.restart(id).catch((e) => toast.error(errorMessage(e)))}>
              <RotateCcw /> Apply & restart
            </Button>
          }
        >
          The server has its {label.toLowerCase()} loaded, so changes are applied when it stops or restarts.
        </Banner>
      )}

      <Card>
        <CardHeader
          title={`${list.entries.length} ${label.toLowerCase()}`}
          description={`${server?.directory ?? ""}\\${list.folder}`}
          actions={
            <>
              <Button size="sm" variant="ghost" disabled={checking} onClick={check}>
                {checking ? <Spinner /> : <RefreshCw />} Check for updates
              </Button>
              <Button size="sm" variant={browse ? "secondary" : "primary"} onClick={() => setBrowse(!browse)}>
                <PackagePlus /> {browse ? "Hide browser" : `Browse ${label.toLowerCase()}`}
              </Button>
            </>
          }
        />
        {list.entries.length === 0 && list.pending.length === 0 ? (
          <EmptyState title={`No ${label.toLowerCase()} installed`} description={`Browse Modrinth and Hangar to add ${label.toLowerCase()}.`} />
        ) : (
          <table className="w-full text-[13px]">
            <thead className="border-b border-border text-left text-xs text-muted">
              <tr>
                <th className="px-4 py-2 font-medium">Name</th>
                <th className="px-4 py-2 font-medium">Version</th>
                <th className="px-4 py-2 font-medium">Source</th>
                <th className="px-4 py-2 font-medium">Size</th>
                <th className="w-10" />
              </tr>
            </thead>
            <tbody>
              {list.entries.map((e) => {
                const update = updates?.find((u) => u.fileName === e.fileName);
                return (
                  <EntryRow
                    key={`${e.enabled}:${e.fileName}`}
                    e={e}
                    list={list}
                    update={update}
                    serverId={id}
                    onRemove={() => setRemoving(e)}
                    onUpdate={() =>
                      update &&
                      e.provider &&
                      e.projectId &&
                      setInstalling({ provider: e.provider, id: e.projectId, name: e.name, version: update.latest.id })
                    }
                  />
                );
              })}
            </tbody>
          </table>
        )}
        {list.pending.length > 0 && (
          <div className="border-t border-border px-4 py-3">
            <p className="mb-2 text-xs font-medium text-muted">Queued changes</p>
            <ul className="space-y-1">
              {list.pending.map((p) => (
                <li key={p.id} className="flex items-center justify-between gap-3 text-xs">
                  <span className="text-fg">
                    {PENDING_LABEL[p.action] ?? p.action}: {p.name} {p.version && <span className="text-muted">{p.version}</span>}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      api.content
                        .discardPending(id, p.id)
                        .then(() => qc.invalidateQueries({ queryKey: qk.content(id) }))
                        .catch((e) => toast.error(errorMessage(e)))
                    }
                  >
                    <Undo2 /> Discard
                  </Button>
                </li>
              ))}
            </ul>
          </div>
        )}
      </Card>

      {browse && (
        <Browser serverId={id} list={list} onInstall={(p) => setInstalling({ provider: p.provider, id: p.id, name: p.name, version: null })} />
      )}

      {installing && <InstallDialog serverId={id} project={installing} versionId={installing.version} onClose={() => setInstalling(null)} />}
      <ConfirmDialog
        open={!!removing}
        onOpenChange={(o) => !o && setRemoving(null)}
        title={`Remove ${removing?.name ?? ""}?`}
        description={
          list.running
            ? "It is removed when the server stops or restarts. The file goes to the server's trash."
            : "The file goes to the server's trash."
        }
        confirmLabel="Remove"
        destructive
        onConfirm={async () => {
          if (!removing) return;
          try {
            await api.content.remove(id, removing.fileName);
          } catch (e) {
            toast.error(errorMessage(e));
          }
        }}
      />
    </PageBody>
  );
}
