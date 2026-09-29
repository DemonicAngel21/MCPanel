import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { FileCode, Info, KeyRound, RotateCcw, Save, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import type { PropertyDto } from "@/bindings/PropertyDto";
import { PageBody } from "@/app/app-shell";
import { PropertyInput } from "@/components/property-input";
import { validateProperty } from "@/lib/properties";
import { Button } from "@/components/ui/button";
import { Banner, Card, Input, SkeletonRows, Spinner, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

const CATEGORIES: { id: string; label: string }[] = [
  { id: "gameplay", label: "Gameplay" },
  { id: "world", label: "World" },
  { id: "network", label: "Network" },
  { id: "performance", label: "Performance" },
  { id: "security", label: "Security" },
  { id: "advanced", label: "Advanced" },
  { id: "other", label: "Other" },
];

function PropertyRow({ p, value, dirty, onChange }: { p: PropertyDto; value: string; dirty: boolean; onChange: (v: string) => void }) {
  const [editingSecret, setEditingSecret] = useState(false);
  const s = p.schema;
  const err = validateProperty(s, value);
  return (
    <div className={cn("grid grid-cols-[minmax(0,1fr)_320px] items-start gap-6 px-4 py-3", dirty && "bg-accent-soft/50")}>
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <label htmlFor={`prop-${p.key}`} className="text-[13px] text-fg">
            {s?.label ?? p.key}
          </label>
          {dirty && <span className="size-1.5 rounded-full bg-accent" title="Changed" />}
          {p.applicability === "unknown" && (
            <Tooltip content="MCPanel could not confirm this setting exists in this Minecraft version.">
              <Info className="size-3.5 text-warning" />
            </Tooltip>
          )}
        </div>
        <p className="selectable font-mono text-[11px] text-faint">{p.key}</p>
        {s?.description && <p className="mt-1 text-xs text-muted">{s.description}</p>}
        {!p.present && (
          <p className="mt-1 text-[11px] text-faint">Not in the file yet — the server uses its default{s?.default ? ` (${s.default})` : ""}.</p>
        )}
      </div>
      <div className="flex flex-col gap-1">
        {s?.sensitive && !editingSecret ? (
          <div className="flex items-center gap-2">
            <span className="flex items-center gap-1.5 text-xs text-muted">
              <KeyRound className="size-3.5" /> {p.hasValue ? "Set (hidden)" : "Not set"}
            </span>
            <Button size="sm" variant="outline" onClick={() => setEditingSecret(true)}>
              Change
            </Button>
          </div>
        ) : (
          <PropertyInput id={`prop-${p.key}`} schema={s} value={value} onChange={onChange} />
        )}
        {err && <p className="text-xs text-danger">{err}</p>}
      </div>
    </div>
  );
}

export function ServerProperties() {
  const id = useServerId();
  const qc = useQueryClient();
  const { data, isLoading, error } = useQuery({ queryKey: qk.serverProperties(id), queryFn: () => api.servers.properties(id) });
  const [changes, setChanges] = useState<Record<string, string>>({});
  const [search, setSearch] = useState("");
  const [saving, setSaving] = useState(false);

  const grouped = useMemo(() => {
    const q = search.trim().toLowerCase();
    const map = new Map<string, PropertyDto[]>();
    for (const p of data?.properties ?? []) {
      if (q && !p.key.includes(q) && !(p.schema?.label.toLowerCase().includes(q) ?? false)) continue;
      const cat = p.schema?.category ?? "other";
      map.set(cat, [...(map.get(cat) ?? []), p]);
    }
    return map;
  }, [data, search]);

  const valueOf = (p: PropertyDto) => changes[p.key] ?? p.value ?? (p.present ? "" : (p.schema?.default ?? ""));
  const dirtyKeys = Object.keys(changes).filter((k) => {
    const p = data?.properties.find((x) => x.key === k);
    return p && (p.schema?.sensitive ? changes[k] !== "" || p.hasValue : changes[k] !== (p.value ?? (p.present ? "" : (p.schema?.default ?? ""))));
  });
  const hasErrors = dirtyKeys.some((k) => {
    const p = data?.properties.find((x) => x.key === k);
    return validateProperty(p?.schema ?? null, changes[k] ?? "") != null;
  });

  const save = async () => {
    setSaving(true);
    try {
      await api.servers.updateProperties(
        id,
        dirtyKeys.map((k) => ({ key: k, value: changes[k] ?? "" })),
      );
      setChanges({});
      await qc.invalidateQueries({ queryKey: qk.serverProperties(id) });
      toast.success(data?.restartRequired ? "Saved. Restart the server to apply the changes." : "Saved");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  if (isLoading)
    return (
      <PageBody className="max-w-5xl">
        <Card>
          <SkeletonRows rows={6} />
        </Card>
      </PageBody>
    );
  if (error || !data)
    return (
      <PageBody>
        <Banner tone="danger" title="Cannot read server.properties">
          {errorMessage(error)}
        </Banner>
      </PageBody>
    );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageBody className="space-y-4">
        <div className="flex items-center gap-2">
          <div className="relative w-72">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-faint" />
            <Input className="pl-8" placeholder="Search settings" value={search} onChange={(e) => setSearch(e.target.value)} />
          </div>
          <div className="flex-1" />
          <Button asChild variant="ghost" size="sm">
            <Link to="/servers/$serverId/edit" params={{ serverId: id }} search={{ path: "server.properties" }}>
              <FileCode /> Edit raw file
            </Link>
          </Button>
        </div>
        {!data.fileExists && (
          <Banner tone="info" icon={<Info />} title="server.properties has not been generated yet">
            Settings shown are those known for Minecraft {data.gameVersion}. After the first start the server writes its full list of settings.
          </Banner>
        )}
        {data.restartRequired && (
          <Banner tone="warning" icon={<RotateCcw />} title="The server is running">
            Changes are saved to the file immediately but take effect after the next restart.
          </Banner>
        )}
        {CATEGORIES.filter((c) => grouped.has(c.id)).map((c) => (
          <Card key={c.id}>
            <div className="border-b border-border px-4 py-2.5 text-xs font-semibold tracking-wide text-muted uppercase">{c.label}</div>
            <div className="divide-y divide-border">
              {grouped.get(c.id)?.map((p) => (
                <PropertyRow
                  key={p.key}
                  p={p}
                  value={valueOf(p)}
                  dirty={dirtyKeys.includes(p.key)}
                  onChange={(v) => setChanges((x) => ({ ...x, [p.key]: v }))}
                />
              ))}
            </div>
          </Card>
        ))}
      </PageBody>
      {dirtyKeys.length > 0 && (
        <div className="flex items-center gap-3 border-t border-border bg-surface px-6 py-3">
          <p className="flex-1 text-xs text-muted">
            {dirtyKeys.length} unsaved change{dirtyKeys.length > 1 ? "s" : ""}: <span className="font-mono">{dirtyKeys.join(", ")}</span>
          </p>
          <Button variant="ghost" onClick={() => setChanges({})}>
            Discard
          </Button>
          <Button variant="primary" onClick={save} disabled={saving || hasErrors}>
            {saving ? <Spinner className="text-accent-fg" /> : <Save />} Save changes
          </Button>
        </div>
      )}
    </div>
  );
}
