import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import {
  AlertTriangle,
  Archive,
  Check,
  ChevronRight,
  Copy,
  Download,
  File,
  FileArchive,
  FilePlus,
  FileText,
  Folder,
  FolderInput,
  FolderPlus,
  Home,
  Link2,
  Lock,
  MoreHorizontal,
  MoveRight,
  PackageOpen,
  Pencil,
  RefreshCw,
  Search,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import type { FileEntryDto } from "@/bindings/FileEntryDto";
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
} from "@/components/ui/overlays";
import { Checkbox, EmptyState, Input, Spinner, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes, formatDateTime } from "@/lib/format";
import { qk } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";
import { serverFilesRoute } from "@/router";
import { useUi } from "@/stores/ui";
import { useServerId } from "./use-server-id";

const EMPTY: Set<string> = new Set();

const TEXT_EXT =
  /\.(txt|log|json|json5|ya?ml|toml|properties|cfg|conf|ini|md|xml|java|js|mjs|ts|sh|bat|cmd|ps1|csv|mcmeta|lang|sk|html|css|secret)$/i;

function isEditable(e: FileEntryDto) {
  return e.kind === "file" && !e.sensitive && (TEXT_EXT.test(e.name) || !e.name.includes("."));
}

function join(dir: string, name: string) {
  return dir ? `${dir}/${name}` : name;
}

function icon(e: FileEntryDto) {
  if (e.kind === "directory") return <Folder className="size-4 text-info" />;
  if (e.kind === "link") return <Link2 className="size-4 text-faint" />;
  if (e.sensitive) return <Lock className="size-4 text-warning" />;
  if (/\.(zip|jar)$/i.test(e.name)) return <FileArchive className="size-4 text-muted" />;
  if (TEXT_EXT.test(e.name)) return <FileText className="size-4 text-muted" />;
  return <File className="size-4 text-muted" />;
}

type NameDialog = { kind: "mkdir" | "create" | "rename" | "zip"; initial: string; target?: FileEntryDto } | null;

/** Folder picker used for Move/Copy destinations (browses the same server). */
function DestinationDialog({
  serverId,
  open,
  onClose,
  onPick,
  title,
}: {
  serverId: string;
  open: boolean;
  onClose: () => void;
  onPick: (dir: string) => void;
  title: string;
}) {
  const [dir, setDir] = useState("");
  const { data } = useQuery({ queryKey: qk.files(serverId, dir), queryFn: () => api.files.list(serverId, dir), enabled: open });
  const parts = dir ? dir.split("/") : [];
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={title}
        description={`Destination: /${dir}`}
        footer={
          <>
            <DialogClose asChild>
              <Button variant="ghost">Cancel</Button>
            </DialogClose>
            <Button variant="primary" onClick={() => onPick(dir)}>
              Choose this folder
            </Button>
          </>
        }
      >
        <div className="mb-2 flex flex-wrap items-center gap-1 text-xs">
          <button className="text-accent hover:underline" onClick={() => setDir("")}>
            root
          </button>
          {parts.map((p, i) => (
            <span key={i} className="flex items-center gap-1">
              <ChevronRight className="size-3 text-faint" />
              <button className="text-accent hover:underline" onClick={() => setDir(parts.slice(0, i + 1).join("/"))}>
                {p}
              </button>
            </span>
          ))}
        </div>
        <ul className="max-h-72 overflow-y-auto rounded-md border border-border">
          {data
            ?.filter((e) => e.kind === "directory")
            .map((e) => (
              <li key={e.path}>
                <button
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[13px] hover:bg-surface-3"
                  onClick={() => setDir(e.path)}
                >
                  <Folder className="size-4 text-info" /> {e.name}
                </button>
              </li>
            ))}
          {data && data.filter((e) => e.kind === "directory").length === 0 && <li className="px-3 py-2 text-xs text-faint">No subfolders</li>}
        </ul>
      </DialogContent>
    </Dialog>
  );
}

export function ServerFiles() {
  const serverId = useServerId();
  const { path } = serverFilesRoute.useSearch();
  const navigate = useNavigate();
  const qc = useQueryClient();
  // Selection is scoped to the folder it was made in.
  const [selection, setSelection] = useState<{ path: string; items: Set<string> }>({ path: "", items: new Set() });
  const selected = selection.path === path ? selection.items : EMPTY;
  const setSelected = (next: Set<string> | ((s: Set<string>) => Set<string>)) =>
    setSelection((cur) => ({ path, items: typeof next === "function" ? next(cur.path === path ? cur.items : EMPTY) : next }));
  const [overwrite, setOverwrite] = useState<{ entry: FileEntryDto; message: string } | null>(null);
  const [nameDialog, setNameDialog] = useState<NameDialog>(null);
  const [nameValue, setNameValue] = useState("");
  const [confirmDelete, setConfirmDelete] = useState<string[] | null>(null);
  const [dest, setDest] = useState<{ mode: "move" | "copy"; paths: string[] } | null>(null);
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);

  const list = useQuery({ queryKey: qk.files(serverId, path), queryFn: () => api.files.list(serverId, path) });
  const searchResults = useQuery({
    queryKey: ["file-search", serverId, path, search],
    queryFn: () => api.files.search(serverId, path, search, 200),
    enabled: search.trim().length >= 2,
  });
  const entries = search.trim().length >= 2 ? searchResults.data : list.data;
  const refresh = () => qc.invalidateQueries({ queryKey: ["servers", serverId, "files"] });

  const go = (p: string) => void navigate({ to: "/servers/$serverId/files", params: { serverId }, search: { path: p } });
  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      toast.success(label);
      await refresh();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  // OS drag-and-drop onto the window uploads into the current folder.
  const runRef = useRef(run);
  useEffect(() => {
    runRef.current = run;
  });
  useEffect(
    () =>
      useUi.subscribe((state, prev) => {
        const dropped = state.droppedFiles;
        if (!dropped?.length || dropped === prev.droppedFiles) return;
        useUi.getState().setDroppedFiles(null);
        void runRef.current(`Uploaded ${dropped.length} item(s)`, () =>
          api.files.import(
            serverId,
            dropped.map((g) => g.token),
            path,
          ),
        );
      }),
    [serverId, path],
  );

  const upload = async (folder: boolean) => {
    const grants = await api.dialog.pickImport(folder).catch((e) => {
      toast.error(errorMessage(e));
      return [];
    });
    if (grants.length)
      await run(`Uploaded ${grants.length} item(s)`, () =>
        api.files.import(
          serverId,
          grants.map((g) => g.token),
          path,
        ),
      );
  };

  const download = async (e: FileEntryDto) => {
    const g = await api.dialog.saveFile(e.name).catch(() => null);
    if (g) await run(`Saved ${e.name}`, () => api.files.export(serverId, e.path, g.token));
  };

  const submitName = async () => {
    if (!nameDialog) return;
    const v = nameValue.trim();
    if (!v) return;
    const d = nameDialog;
    setNameDialog(null);
    if (d.kind === "mkdir") await run("Folder created", () => api.files.mkdir(serverId, join(path, v)));
    else if (d.kind === "create") {
      await run("File created", () => api.files.create(serverId, join(path, v)));
      void navigate({ to: "/servers/$serverId/edit", params: { serverId }, search: { path: join(path, v) } });
    } else if (d.kind === "rename" && d.target) await run("Renamed", () => api.files.rename(serverId, d.target!.path, v));
    else if (d.kind === "zip") {
      const paths = [...selected];
      await run("Archive created", async () => {
        const r = await api.files.zip(serverId, paths, v.toLowerCase().endsWith(".zip") ? v : `${v}.zip`);
        if (r.skippedSensitive > 0) toast.warning(`${r.skippedSensitive} protected key file(s) were not added to the archive`);
      });
    }
  };

  const openName = (d: NonNullable<NameDialog>) => {
    setNameDialog(d);
    setNameValue(d.initial);
  };

  const unzip = async (e: FileEntryDto) => {
    try {
      setBusy(true);
      const r = await api.files.unzip(serverId, e.path, path, false);
      toast.success(`Extracted ${r.files} file(s)`);
      await refresh();
    } catch (err) {
      const code = (err as { code?: string }).code;
      if (code === "PATH_EXISTS") setOverwrite({ entry: e, message: errorMessage(err) });
      else toast.error(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  type FileCategory = "all" | "configs" | "logs" | "jars" | "folders";

  const crumbs = path ? path.split("/") : [];
  const [category, setCategory] = useState<FileCategory>("all");
  const [copiedPath, setCopiedPath] = useState(false);

  const copyPath = async () => {
    try {
      await navigator.clipboard.writeText(`/${path}`);
      setCopiedPath(true);
      toast.success("Folder path copied");
      setTimeout(() => setCopiedPath(false), 2000);
    } catch {
      toast.error("Failed to copy path");
    }
  };

  const counts = useMemo(() => {
    if (!entries) return { all: 0, configs: 0, logs: 0, jars: 0, folders: 0 };
    let configs = 0;
    let logs = 0;
    let jars = 0;
    let folders = 0;
    for (const e of entries) {
      if (e.kind === "directory") folders++;
      else if (/\.(ya?ml|properties|json|json5|toml|cfg|conf|ini|env)$/i.test(e.name)) configs++;
      else if (/\.(log|txt|gz)$/i.test(e.name)) logs++;
      else if (/\.(jar|zip)$/i.test(e.name)) jars++;
    }
    return { all: entries.length, configs, logs, jars, folders };
  }, [entries]);

  const filtered = useMemo(() => {
    if (!entries) return [];
    if (category === "configs")
      return entries.filter((e) => e.kind === "file" && /\.(ya?ml|properties|json|json5|toml|cfg|conf|ini|env)$/i.test(e.name));
    if (category === "logs") return entries.filter((e) => e.kind === "file" && /\.(log|txt|gz)$/i.test(e.name));
    if (category === "jars") return entries.filter((e) => e.kind === "file" && /\.(jar|zip)$/i.test(e.name));
    if (category === "folders") return entries.filter((e) => e.kind === "directory");
    return entries;
  }, [entries, category]);

  const sorted = useMemo(() => filtered, [filtered]);
  const allSelected = !!sorted.length && sorted.every((e) => selected.has(e.path));
  const selectedList = [...selected];

  return (
    <div className="flex min-h-0 flex-1 flex-col px-6 py-4">
      <div className="mb-3 flex items-center gap-2">
        <nav aria-label="Folder path" className="flex min-w-0 flex-1 items-center gap-1 text-[13px]">
          <button
            onClick={() => go("")}
            aria-label="Server folder"
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-muted hover:bg-surface-3 hover:text-fg"
          >
            <Home className="size-3.5" />
          </button>
          {crumbs.map((c, i) => (
            <span key={i} className="flex min-w-0 items-center gap-1">
              <ChevronRight className="size-3.5 shrink-0 text-faint" />
              <button
                onClick={() => go(crumbs.slice(0, i + 1).join("/"))}
                className="truncate rounded px-1.5 py-0.5 text-muted hover:bg-surface-3 hover:text-fg"
              >
                {c}
              </button>
            </span>
          ))}
          <Tooltip content={copiedPath ? "Copied!" : "Copy folder path"}>
            <button
              type="button"
              onClick={() => void copyPath()}
              className="ml-1 rounded p-1 text-faint hover:bg-surface-3 hover:text-fg"
              aria-label="Copy folder path"
            >
              {copiedPath ? <Check className="text-success size-3" /> : <Copy className="size-3" />}
            </button>
          </Tooltip>
        </nav>
        <div className="relative w-56">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-faint" />
          <Input
            className="pr-7 pl-8"
            placeholder="Search names in this folder"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setSearch("");
            }}
          />
          {search && (
            <button
              type="button"
              onClick={() => setSearch("")}
              className="absolute top-1/2 right-2 -translate-y-1/2 text-muted hover:text-fg"
              aria-label="Clear search"
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>
        <Tooltip content="New folder">
          <Button variant="ghost" size="icon" onClick={() => openName({ kind: "mkdir", initial: "" })} aria-label="New folder">
            <FolderPlus />
          </Button>
        </Tooltip>
        <Tooltip content="New file">
          <Button variant="ghost" size="icon" onClick={() => openName({ kind: "create", initial: "" })} aria-label="New file">
            <FilePlus />
          </Button>
        </Tooltip>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline">
              <Upload /> Upload
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            <DropdownMenuItem onSelect={() => upload(false)}>
              <File /> Files…
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => upload(true)}>
              <FolderInput /> Folder…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Tooltip content="Refresh">
          <Button variant="ghost" size="icon" onClick={() => refresh()} aria-label="Refresh">
            {busy || list.isFetching ? <Spinner /> : <RefreshCw />}
          </Button>
        </Tooltip>
      </div>

      <div className="mb-2.5 flex items-center gap-1.5 overflow-x-auto text-xs">
        {(
          [
            { id: "all", label: "All", count: counts.all },
            { id: "configs", label: "Configs", count: counts.configs },
            { id: "logs", label: "Logs", count: counts.logs },
            { id: "jars", label: "Jars & Zips", count: counts.jars },
            { id: "folders", label: "Folders", count: counts.folders },
          ] as const
        ).map((tab) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => setCategory(tab.id)}
            className={cn(
              "flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs transition-colors",
              category === tab.id ? "bg-accent font-medium text-accent-fg" : "bg-surface-2 text-muted hover:bg-surface-3 hover:text-fg",
            )}
          >
            <span>{tab.label}</span>
            {tab.count > 0 && <span className={cn("text-[10px]", category === tab.id ? "text-accent-fg/80" : "text-faint")}>{tab.count}</span>}
          </button>
        ))}
      </div>

      {selected.size > 0 && (
        <div className="mb-2 flex items-center gap-2 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-xs">
          <span className="text-fg">{selected.size} selected</span>
          <div className="flex-1" />
          <Button
            size="sm"
            variant="ghost"
            onClick={() => openName({ kind: "zip", initial: selected.size === 1 ? `${selectedList[0]?.split("/").pop()}.zip` : "archive.zip" })}
          >
            <Archive /> Zip
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setDest({ mode: "copy", paths: selectedList })}>
            <Copy /> Copy to…
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setDest({ mode: "move", paths: selectedList })}>
            <MoveRight /> Move to…
          </Button>
          <Button size="sm" variant="danger-outline" onClick={() => setConfirmDelete(selectedList)}>
            <Trash2 /> Delete
          </Button>
          <Button size="icon-sm" variant="ghost" onClick={() => setSelected(new Set())} aria-label="Clear selection">
            <X />
          </Button>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-border bg-surface">
        {list.isError ? (
          <EmptyState tone="danger" icon={<AlertTriangle />} title="Cannot open this folder" description={errorMessage(list.error)} />
        ) : sorted.length === 0 ? (
          <EmptyState
            icon={<Folder />}
            title={search ? "No matches" : category !== "all" ? `No ${category} found` : "This folder is empty"}
            description={
              search
                ? undefined
                : category !== "all"
                  ? "Try switching to All to see other items in this folder."
                  : "Upload files or drag them onto the window."
            }
          />
        ) : (
          <table className="w-full text-[13px]">
            <thead className="sticky top-0 bg-surface">
              <tr className="border-b border-border text-left text-xs text-muted">
                <th className="w-8 px-3 py-2">
                  <Checkbox
                    checked={allSelected}
                    onCheckedChange={(c) => setSelected(c ? new Set(sorted.map((e) => e.path)) : new Set())}
                    aria-label="Select all"
                  />
                </th>
                <th className="py-2 font-medium">Name</th>
                <th className="w-28 py-2 font-medium">Size</th>
                <th className="w-44 py-2 font-medium">Modified</th>
                <th className="w-10">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {sorted.map((e) => (
                <tr
                  key={e.path}
                  className={cn("group hover:bg-surface-2", selected.has(e.path) && "bg-accent-soft")}
                  onDoubleClick={() => {
                    if (e.kind === "directory") go(e.path);
                    else if (isEditable(e)) void navigate({ to: "/servers/$serverId/edit", params: { serverId }, search: { path: e.path } });
                  }}
                >
                  <td className="px-3 py-1.5">
                    <Checkbox
                      checked={selected.has(e.path)}
                      onCheckedChange={(c) =>
                        setSelected((s) => {
                          const n = new Set(s);
                          if (c) n.add(e.path);
                          else n.delete(e.path);
                          return n;
                        })
                      }
                      aria-label={`Select ${e.name}`}
                    />
                  </td>
                  <td className="py-1.5">
                    <div className="flex items-center gap-2">
                      {icon(e)}
                      {e.kind === "directory" ? (
                        <button className="truncate text-fg hover:underline" onClick={() => go(e.path)}>
                          {search ? e.path : e.name}
                        </button>
                      ) : isEditable(e) ? (
                        <Link
                          to="/servers/$serverId/edit"
                          params={{ serverId }}
                          search={{ path: e.path }}
                          className="truncate text-fg hover:underline"
                        >
                          {search ? e.path : e.name}
                        </Link>
                      ) : (
                        <span className="truncate text-fg">{search ? e.path : e.name}</span>
                      )}
                      {e.sensitive && <span className="rounded bg-warning-soft px-1.5 text-[10px] text-warning">protected key</span>}
                      {e.kind === "link" && <span className="text-[11px] text-faint">link (not followed)</span>}
                    </div>
                  </td>
                  <td className="py-1.5 text-muted tabular-nums">{e.kind === "file" ? formatBytes(e.size) : ""}</td>
                  <td className="py-1.5 text-muted">{formatDateTime(e.modified)}</td>
                  <td className="pr-2 text-right">
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="opacity-0 group-hover:opacity-100 data-[state=open]:opacity-100"
                          aria-label="Actions"
                        >
                          <MoreHorizontal />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent>
                        {isEditable(e) && (
                          <DropdownMenuItem
                            onSelect={() => void navigate({ to: "/servers/$serverId/edit", params: { serverId }, search: { path: e.path } })}
                          >
                            <FileText /> Edit
                          </DropdownMenuItem>
                        )}
                        {e.kind === "file" && !e.sensitive && (
                          <DropdownMenuItem onSelect={() => download(e)}>
                            <Download /> Download…
                          </DropdownMenuItem>
                        )}
                        {e.kind === "file" && /\.zip$/i.test(e.name) && (
                          <DropdownMenuItem onSelect={() => unzip(e)}>
                            <PackageOpen /> Extract here
                          </DropdownMenuItem>
                        )}
                        <DropdownMenuItem onSelect={() => openName({ kind: "rename", initial: e.name, target: e })}>
                          <Pencil /> Rename
                        </DropdownMenuItem>
                        <DropdownMenuItem onSelect={() => setDest({ mode: "copy", paths: [e.path] })}>
                          <Copy /> Copy to…
                        </DropdownMenuItem>
                        <DropdownMenuItem onSelect={() => setDest({ mode: "move", paths: [e.path] })}>
                          <MoveRight /> Move to…
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem destructive onSelect={() => setConfirmDelete([e.path])}>
                          <Trash2 /> Delete
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      <p className="mt-2 text-[11px] text-faint">
        Deleted items go to the server's .mcpanel/trash folder. Links and junctions are shown but never followed. Protected key files (such as
        Floodgate keys) cannot be opened or downloaded.
      </p>

      <Dialog open={!!nameDialog} onOpenChange={(o) => !o && setNameDialog(null)}>
        <DialogContent
          title={{ mkdir: "New folder", create: "New file", rename: "Rename", zip: "Create archive" }[nameDialog?.kind ?? "mkdir"]}
          footer={
            <>
              <DialogClose asChild>
                <Button variant="ghost">Cancel</Button>
              </DialogClose>
              <Button variant="primary" onClick={submitName} disabled={!nameValue.trim()}>
                {nameDialog?.kind === "rename" ? "Rename" : "Create"}
              </Button>
            </>
          }
        >
          <Input
            autoFocus
            value={nameValue}
            onChange={(e) => setNameValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void submitName()}
            placeholder={nameDialog?.kind === "zip" ? "archive.zip" : "Name"}
          />
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={!!confirmDelete}
        onOpenChange={(o) => !o && setConfirmDelete(null)}
        title={`Delete ${confirmDelete?.length ?? 0} item(s)?`}
        description="Items are moved to this server's .mcpanel/trash folder, where you can recover them from Windows Explorer."
        confirmLabel="Delete"
        destructive
        onConfirm={async () => {
          const p = confirmDelete ?? [];
          setSelected(new Set());
          await run("Moved to trash", () => api.files.delete(serverId, p, false));
        }}
      >
        <ul className="max-h-40 overflow-y-auto font-mono text-xs text-muted">
          {confirmDelete?.map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>
      </ConfirmDialog>

      <ConfirmDialog
        open={!!overwrite}
        onOpenChange={(o) => !o && setOverwrite(null)}
        title="Overwrite existing files?"
        description={overwrite?.message}
        confirmLabel="Extract and overwrite"
        destructive
        onConfirm={async () => {
          const o = overwrite;
          if (o) await run("Extracted (overwrote existing files)", () => api.files.unzip(serverId, o.entry.path, path, true));
        }}
      />

      <DestinationDialog
        serverId={serverId}
        open={!!dest}
        title={dest?.mode === "move" ? "Move to…" : "Copy to…"}
        onClose={() => setDest(null)}
        onPick={async (dir) => {
          const d = dest;
          setDest(null);
          setSelected(new Set());
          if (!d) return;
          if (d.mode === "move") await run("Moved", () => api.files.move(serverId, d.paths, dir));
          else await run("Copied", () => api.files.copy(serverId, d.paths, dir));
        }}
      />
    </div>
  );
}
