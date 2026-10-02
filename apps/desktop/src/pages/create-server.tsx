import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { AlertTriangle, Check, ExternalLink, FolderOpen, LayoutTemplate, ShieldCheck, ShieldAlert } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import type { GrantDto } from "@/bindings/GrantDto";
import type { JavaCompatibilityDto } from "@/bindings/JavaCompatibilityDto";
import { PageBody, PageHeader } from "@/app/app-shell";
import { MemoryRange } from "@/components/memory-slider";
import { PropertyInput } from "@/components/property-input";
import { SoftwareMark } from "@/components/software-mark";
import { LOCATION_WARNING_TEXT } from "@/lib/location";
import { validateProperty } from "@/lib/properties";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Badge, Banner, Card, CardHeader, Checkbox, Field, Input, Progress, Spinner, Switch, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { qk, useJava, useServers, useSoftware, useSystemMetrics, useTemplates } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";
import { useUi } from "@/stores/ui";

const STEPS = ["Basics", "Software", "Java & memory", "World & gameplay", "Review"] as const;

/** Properties offered in the wizard (only those that exist for the chosen version are shown). */
const WIZARD_KEYS = [
  "motd",
  "level-name",
  "level-seed",
  "gamemode",
  "difficulty",
  "hardcore",
  "pvp",
  "online-mode",
  "white-list",
  "max-players",
  "view-distance",
  "simulation-distance",
  "server-port",
];

function compatLabel(c: JavaCompatibilityDto | undefined): { text: string; tone: "success" | "warning" | "danger" | "neutral" } {
  if (!c) return { text: "Unknown", tone: "neutral" };
  switch (c.status) {
    case "compatible":
      return { text: "Compatible", tone: "success" };
    case "too_old":
      return { text: `Needs Java ${c.required}+`, tone: "danger" };
    case "newer_than_recommended":
      return { text: `Newer than Java ${c.recommended}`, tone: "warning" };
    default:
      return { text: "Not validated", tone: "neutral" };
  }
}

export function CreateServerPage() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [step, setStep] = useState(0);
  const [name, setName] = useState("My Server");
  const [parent, setParent] = useState<GrantDto | null>(null);
  const search = useSearch({ strict: false }) as { template?: string };
  const { data: templates } = useTemplates();
  const template = templates?.find((t) => t.id === search.template);
  const [softwareChoice, setSoftwareId] = useState<string | null>(null);
  const { data: softwareList } = useSoftware();
  const templateSoftware = template?.software.find((id) => softwareList?.some((s) => s.id === id && s.supported));
  const softwareId = softwareChoice ?? templateSoftware ?? "paper";
  const [snapshots, setSnapshots] = useState(false);
  const [versionChoice, setVersion] = useState<string | undefined>();
  // Build and Java choices are remembered per software+version; otherwise defaults apply.
  const [buildChoice, setBuildChoice] = useState<{ key: string; value: string } | null>(null);
  const [javaChoice, setJavaChoice] = useState<{ key: string; value: string } | null>(null);
  const [minMemChoice, setMinMem] = useState<number | null>(null);
  const [pluginsOff, setPluginsOff] = useState<string[]>([]);
  const [maxMemChoice, setMaxMem] = useState<number | null>(null);
  const [useFlags, setUseFlags] = useState(true);
  const [props, setProps] = useState<Record<string, string>>({});
  const [eula, setEula] = useState(false);
  const [jobId, setJobId] = useState<string | null>(null);
  const [appliedTemplate, setAppliedTemplate] = useState<{ id: string; name: string; plugins: string[] } | null>(null);
  const job = useUi((s) => (jobId ? s.jobs[jobId] : undefined));

  const { data: software } = useSoftware();
  const { data: java } = useJava();
  const { data: servers } = useServers();
  const { data: metrics } = useSystemMetrics();
  const totalMb = metrics?.current ? Math.floor(metrics.current.memoryTotalBytes / 1024 / 1024) : null;

  const location = useQuery({
    queryKey: ["check-location", name, parent?.token],
    queryFn: () => api.servers.checkLocation(name, parent?.token ?? null),
    enabled: name.trim().length > 0,
    retry: false,
  });
  const versions = useQuery({
    queryKey: qk.versions(softwareId, snapshots),
    queryFn: () => api.software.versions(softwareId, snapshots),
    retry: 1,
  });
  // Default version = newest in the list.
  const version = versionChoice && versions.data?.some((v) => v.id === versionChoice) ? versionChoice : versions.data?.[0]?.id;
  const choiceKey = `${softwareId}|${version ?? ""}`;
  const build = buildChoice?.key === choiceKey ? buildChoice.value : "latest";
  const setBuild = (value: string) => setBuildChoice({ key: choiceKey, value });

  const builds = useQuery({
    queryKey: qk.builds(softwareId, version ?? ""),
    queryFn: () => api.software.builds(softwareId, version ?? ""),
    enabled: !!version,
  });
  const preview = useQuery({
    queryKey: qk.preview(softwareId, version ?? "", build === "latest" ? null : build),
    queryFn: () => api.software.preview(softwareId, version ?? "", build === "latest" ? null : build),
    enabled: !!version && step >= 2,
    retry: false,
  });
  const schema = useQuery({
    queryKey: qk.propertySchema(version ?? ""),
    queryFn: () => api.software.propertySchema(version ?? ""),
    enabled: !!version,
  });

  // Default memory: the template's range, else half the system RAM capped at 4 GB — never
  // more than the computer can spare.
  const ramCap = totalMb ? Math.max(1024, Math.floor((totalMb - 2048) / 512) * 512) : null;
  const maxMem =
    maxMemChoice ??
    (template?.memoryMb
      ? Math.min(template.memoryMb.max, ramCap ?? template.memoryMb.max)
      : totalMb
        ? Math.max(1024, Math.min(4096, Math.floor(totalMb / 2 / 512) * 512))
        : 4096);
  const minMem = minMemChoice ?? Math.min(template?.memoryMb?.min ?? 1024, maxMem);

  // Default Java: the closest compatible runtime (recommended major, else the oldest compatible).
  const compat = useMemo(() => Object.fromEntries((preview.data?.java ?? []).map((j) => [j.javaRuntimeId, j.compatibility])), [preview.data]);
  const defaultJava = useMemo(() => {
    if (!java || !preview.data) return undefined;
    const candidates = java.filter((j) => j.valid && compat[j.id]?.status === "compatible").sort((a, b) => a.major - b.major);
    const rec = preview.data.javaRecommendedMajor;
    return (candidates.find((j) => j.major === rec) ?? candidates[0])?.id;
  }, [java, preview.data, compat]);
  const javaId = javaChoice?.key === choiceKey ? javaChoice.value : defaultJava;
  const setJavaId = (value: string) => setJavaChoice({ key: choiceKey, value });

  const schemaByKey = useMemo(() => Object.fromEntries((schema.data ?? []).map((p) => [p.key, p.schema])), [schema.data]);
  const wizardKeys = WIZARD_KEYS.filter((k) => k in schemaByKey);
  const resolved = useQuery({
    queryKey: ["template-resolve", template?.id, softwareId, version],
    queryFn: () => api.templates.resolve(template?.id ?? "", softwareId, version ?? ""),
    enabled: !!template && !!version && (template?.software.includes(softwareId) ?? false),
  });
  const templateValues = useMemo(() => Object.fromEntries((resolved.data?.properties ?? []).map((p) => [p.key, p.value])), [resolved.data]);
  const value = (k: string) => props[k] ?? templateValues[k] ?? schemaByKey[k]?.default ?? "";
  const templateExtras = (resolved.data?.properties ?? []).filter((p) => !wizardKeys.includes(p.key));
  const templatePlugins = (resolved.data?.plugins ?? []).filter((p) => !pluginsOff.includes(p.project));
  const portInUse = servers?.find((s) => String(s.port ?? 25565) === value("server-port"));

  const propErrors = wizardKeys.map((k) => validateProperty(schemaByKey[k] ?? null, value(k))).filter(Boolean);
  const selectedJava = java?.find((j) => j.id === javaId);
  const javaOk = !!selectedJava && compat[selectedJava.id]?.status !== "too_old";
  const memOk = minMem >= 0 && maxMem >= 512 && minMem <= maxMem && (!totalMb || maxMem <= totalMb);

  const canNext = [name.trim().length > 0 && location.isSuccess, !!version, preview.isSuccess && javaOk && memOk, propErrors.length === 0, eula][
    step
  ];

  const create = async () => {
    if (!version || !javaId) return;
    const properties = [...wizardKeys.map((k) => ({ key: k, value: value(k) })), ...templateExtras].filter((p) => p.value !== "");
    try {
      const id = await api.servers.create({
        name,
        parentDirectoryGrant: parent?.token ?? null,
        softwareId,
        gameVersion: version,
        build: build === "latest" ? null : build,
        javaRuntimeId: javaId,
        minMemoryMb: minMem,
        maxMemoryMb: maxMem,
        jvmArgs: useFlags ? (preview.data?.recommendedJvmFlags ?? []) : [],
        properties,
        acceptEula: eula,
      });
      useUi.getState().upsertJob({ id, kind: "server.create", serverId: null, status: "running", progress: 0, message: "Starting" });
      if (template) setAppliedTemplate({ id: template.id, name: template.name, plugins: templatePlugins.map((p) => p.project) });
      setJobId(id);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  // Navigate when the provisioning job succeeds (failures are rendered below).
  useEffect(() => {
    if (!jobId || job?.status !== "succeeded") return;
    void api.jobs.get(jobId).then((j) => {
      const sid = (j.result as { serverId?: string } | null)?.serverId;
      void qc.invalidateQueries({ queryKey: qk.servers });
      if (!sid) return;
      const go = () => void navigate({ to: "/servers/$serverId", params: { serverId: sid } });
      if (!appliedTemplate) return go();
      api.templates
        .apply(sid, appliedTemplate.id, appliedTemplate.plugins)
        .then(() => toast.success(`Applied the ${appliedTemplate.name} template`))
        .catch((e) => toast.error(`The template could not be applied fully: ${errorMessage(e)}`))
        .finally(go);
    });
  }, [job?.status, jobId, navigate, qc, appliedTemplate]);
  const jobFailed = !!jobId && !!job && job.status !== "running" && job.status !== "succeeded";
  const sw = software?.find((s) => s.id === softwareId);

  return (
    <>
      <PageHeader title="Create server" description="Set up a new Minecraft Java server. You can change everything later.">
        <ol className="flex gap-1 pb-3">
          {STEPS.map((s, i) => (
            <li key={s}>
              <button
                type="button"
                disabled={i > step || !!jobId}
                onClick={() => setStep(i)}
                className={cn(
                  "flex items-center gap-2 rounded-md px-2.5 py-1 text-xs",
                  i === step ? "bg-accent-soft text-accent" : i < step ? "text-fg hover:bg-surface-3" : "text-faint",
                )}
              >
                <span
                  className={cn(
                    "flex size-4 items-center justify-center rounded-full text-[10px]",
                    i < step ? "bg-accent text-accent-fg" : "border border-current",
                  )}
                >
                  {i < step ? <Check className="size-3" /> : i + 1}
                </span>
                {s}
              </button>
            </li>
          ))}
        </ol>
      </PageHeader>
      <PageBody>
        <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_320px]">
          <div className="space-y-5">
            {template && !jobId && (
              <Banner
                tone="info"
                icon={<LayoutTemplate />}
                title={`Template: ${template.name}`}
                actions={
                  <Button asChild size="sm" variant="ghost">
                    <Link to="/servers/new">Start without template</Link>
                  </Button>
                }
              >
                {template.description} The settings below are filled in from the template; you can change them.
              </Banner>
            )}
            {jobId ? (
              <Card className="space-y-3 p-6">
                <p className="text-sm font-medium text-fg">{jobFailed ? `Could not create ${name}` : `Creating ${name}…`}</p>
                <Progress value={job?.progress ?? null} />
                <p className={jobFailed ? "text-xs text-danger" : "text-xs text-muted"}>{job?.message ?? "Preparing"}</p>
                {jobFailed && (
                  <div className="flex justify-end">
                    <Button variant="outline" onClick={() => setJobId(null)}>
                      Back to the wizard
                    </Button>
                  </div>
                )}
              </Card>
            ) : (
              <>
                {step === 0 && (
                  <Card className="space-y-4 p-5">
                    <Field label="Server name">
                      <Input value={name} onChange={(e) => setName(e.target.value)} maxLength={64} autoFocus />
                    </Field>
                    <Field
                      label="Location"
                      error={location.isError ? errorMessage(location.error) : undefined}
                      hint="A new folder is created here for the server. By default MCPanel uses your user folder, not Documents (which is often synced by OneDrive)."
                    >
                      <div className="flex gap-2">
                        <Input readOnly value={location.data?.directory ?? ""} className="font-mono text-xs" />
                        <Button
                          variant="outline"
                          onClick={async () => {
                            const g = await api.dialog.pickFolder("Choose where to create the server folder").catch((e) => {
                              toast.error(errorMessage(e));
                              return null;
                            });
                            if (g) setParent(g);
                          }}
                        >
                          <FolderOpen /> Change
                        </Button>
                      </div>
                    </Field>
                    {location.data?.warnings.map((w) => (
                      <Banner key={w} tone="warning" icon={<AlertTriangle />} title="Check this location">
                        {LOCATION_WARNING_TEXT[w] ?? w}
                      </Banner>
                    ))}
                  </Card>
                )}

                {step === 1 && (
                  <Card className="space-y-4 p-5">
                    <Field label="Server software">
                      <div className="grid grid-cols-3 gap-2">
                        {software?.map((s) => (
                          <button
                            key={s.id}
                            type="button"
                            onClick={() => setSoftwareId(s.id)}
                            className={cn(
                              "rounded-lg border p-3 text-left transition-colors",
                              softwareId === s.id ? "border-accent bg-accent-soft" : "border-border hover:border-border-strong",
                            )}
                          >
                            <span className="mb-2 flex items-center gap-2">
                              <SoftwareMark softwareId={s.id} name={s.displayName} size="sm" />
                              <span className="text-[13px] font-semibold text-fg">{s.displayName}</span>
                            </span>
                            <span className="line-clamp-2 text-xs text-muted">{s.description}</span>
                          </button>
                        ))}
                      </div>
                    </Field>
                    <div className="grid grid-cols-2 gap-4">
                      <Field label="Minecraft version" error={versions.isError ? errorMessage(versions.error) : undefined}>
                        <Select
                          value={version}
                          onValueChange={setVersion}
                          placeholder={versions.isLoading ? "Loading…" : "Select version"}
                          options={(versions.data ?? []).map((v) => ({
                            value: v.id,
                            label: v.id,
                            hint: v.kind !== "release" ? v.kind.replace("_", " ") : undefined,
                          }))}
                        />
                      </Field>
                      <Field label="Build" hint={sw?.id === "vanilla" ? "Vanilla has no separate builds." : undefined}>
                        <Select
                          value={build}
                          onValueChange={setBuild}
                          disabled={sw?.id === "vanilla" || !builds.data?.length}
                          options={[
                            { value: "latest", label: "Latest stable" },
                            ...(builds.data ?? [])
                              .slice(0, 50)
                              .map((b) => ({ value: b.id, label: `#${b.id}`, hint: b.channel !== "stable" ? b.channel : undefined })),
                          ]}
                        />
                      </Field>
                    </div>
                    <label className="flex items-center gap-2 text-xs text-muted">
                      <Switch checked={snapshots} onCheckedChange={setSnapshots} /> Show snapshots and pre-releases
                    </label>
                  </Card>
                )}

                {step === 2 && (
                  <Card className="space-y-4 p-5">
                    {preview.isLoading && (
                      <p className="flex items-center gap-2 text-xs text-muted">
                        <Spinner /> Resolving {sw?.displayName} {version}…
                      </p>
                    )}
                    {preview.isError && (
                      <Banner tone="danger" title="Could not resolve this version">
                        {errorMessage(preview.error)}
                      </Banner>
                    )}
                    {preview.data && (
                      <>
                        <p className="text-xs text-muted">
                          {sw?.displayName} {version} {preview.data.build ? `build #${preview.data.build}` : ""} requires{" "}
                          <span className="font-medium text-fg">Java {preview.data.javaMinMajor} or newer</span>.
                        </p>
                        <Field label="Java runtime">
                          <div className="space-y-1.5">
                            {java
                              ?.filter((j) => j.valid)
                              .map((j) => {
                                const c = compatLabel(compat[j.id]);
                                return (
                                  <button
                                    key={j.id}
                                    type="button"
                                    disabled={c.tone === "danger"}
                                    onClick={() => setJavaId(j.id)}
                                    className={cn(
                                      "flex w-full items-center gap-3 rounded-lg border px-3 py-2 text-left disabled:opacity-50",
                                      javaId === j.id ? "border-accent bg-accent-soft" : "border-border hover:border-border-strong",
                                    )}
                                  >
                                    <span className="font-semibold text-fg">Java {j.major}</span>
                                    <span className="min-w-0 flex-1 truncate text-xs text-muted">
                                      {j.vendor} · {j.path}
                                    </span>
                                    <Badge tone={c.tone}>{c.text}</Badge>
                                  </button>
                                );
                              })}
                            {java?.filter((j) => j.valid).length === 0 && (
                              <Banner tone="warning" title="No usable Java runtime">
                                Install Java {preview.data.javaMinMajor}+ and detect it on the Java page.
                              </Banner>
                            )}
                          </div>
                        </Field>
                      </>
                    )}
                    <div className="space-y-1.5">
                      <p className="text-xs font-medium text-muted">Memory</p>
                      <MemoryRange
                        minMb={minMem}
                        maxMb={maxMem}
                        totalMb={totalMb}
                        onChange={(lo, hi) => {
                          setMinMem(lo);
                          setMaxMem(hi);
                        }}
                      />
                      {!memOk && (
                        <p className="text-xs text-danger">
                          Maximum must be at least 512 MB, not below the minimum, and fit in this computer's memory.
                        </p>
                      )}
                    </div>
                    {preview.data && preview.data.recommendedJvmFlags.length > 0 && (
                      <label className="flex items-start gap-2 text-xs text-muted">
                        <Checkbox checked={useFlags} onCheckedChange={(c) => setUseFlags(c === true)} className="mt-0.5" />
                        <span>
                          Use the JVM flags recommended by {sw?.displayName} ({preview.data.recommendedJvmFlags.length} G1GC tuning flags). You can
                          edit them later.
                        </span>
                      </label>
                    )}
                  </Card>
                )}

                {step === 3 && (
                  <Card className="grid grid-cols-2 gap-4 p-5">
                    {schema.isLoading && <Spinner />}
                    {wizardKeys.map((k) => {
                      const s = schemaByKey[k] ?? null;
                      const err = validateProperty(s, value(k));
                      const wide = s?.kind === "text" || k === "level-seed";
                      return (
                        <Field
                          key={k}
                          label={s?.label ?? k}
                          className={wide ? "col-span-2" : undefined}
                          error={
                            err ??
                            (k === "server-port" && portInUse
                              ? `Port also used by "${portInUse.name}" — both cannot run at the same time.`
                              : undefined)
                          }
                          hint={s?.description ?? undefined}
                        >
                          <PropertyInput id={`p-${k}`} schema={s} value={value(k)} onChange={(v) => setProps((p) => ({ ...p, [k]: v }))} />
                        </Field>
                      );
                    })}
                    {value("online-mode") === "false" && (
                      <div className="col-span-2">
                        <Banner tone="warning" title="Cracked / Offline mode is selected">
                          Unauthenticated players can join with any username. For security, enable the whitelist or plan to install an authentication
                          plugin.
                        </Banner>
                      </div>
                    )}
                  </Card>
                )}

                {step === 4 && (
                  <Card className="space-y-4 p-5">
                    {template && (
                      <div className="space-y-2 rounded-lg border border-border bg-surface-2 p-3 text-xs">
                        <p className="font-medium text-fg">From the {template.name} template</p>
                        {templateExtras.length > 0 && (
                          <p className="text-muted">Also sets: {templateExtras.map((p) => `${p.key}=${p.value}`).join(", ")}</p>
                        )}
                        <p className="text-muted">
                          {template.backupIntervalMinutes
                            ? `Backups every ${template.backupIntervalMinutes >= 60 ? `${template.backupIntervalMinutes / 60} hours` : `${template.backupIntervalMinutes} minutes`}`
                            : "No backup schedule"}
                          {" · "}
                          {template.autoRestart ? "restart after a crash" : "no automatic restart"}
                        </p>
                        {(resolved.data?.plugins ?? []).map((p) => (
                          <label key={p.project} className="flex items-start gap-2 text-fg">
                            <Checkbox
                              className="mt-0.5"
                              checked={!pluginsOff.includes(p.project)}
                              onCheckedChange={(c) => setPluginsOff((off) => (c === true ? off.filter((x) => x !== p.project) : [...off, p.project]))}
                            />
                            <span>
                              Install {p.name} <span className="text-muted">— {p.reason}</span>
                            </span>
                          </label>
                        ))}
                        {resolved.data?.notes.map((n) => (
                          <p key={n} className="text-warning">
                            {n}
                          </p>
                        ))}
                      </div>
                    )}
                    <dl className="grid grid-cols-[160px_1fr] gap-x-4 gap-y-2 text-[13px]">
                      <dt className="text-muted">Name</dt>
                      <dd className="text-fg">{name}</dd>
                      <dt className="text-muted">Folder</dt>
                      <dd className="selectable font-mono text-xs text-fg">{location.data?.directory}</dd>
                      <dt className="text-muted">Software</dt>
                      <dd className="text-fg">
                        {sw?.displayName} {version} {preview.data?.build ? `#${preview.data.build}` : ""}
                        {preview.data?.buildChannel && preview.data.buildChannel !== "stable" && (
                          <Badge tone="warning" className="ml-2">
                            {preview.data.buildChannel}
                          </Badge>
                        )}
                      </dd>
                      <dt className="text-muted">Java</dt>
                      <dd className="text-fg">
                        Java {selectedJava?.major} ({selectedJava?.vendor})
                      </dd>
                      <dt className="text-muted">Memory</dt>
                      <dd className="text-fg">
                        {minMem} – {maxMem} MB
                      </dd>
                      <dt className="text-muted">Download</dt>
                      <dd className="flex flex-wrap items-center gap-1.5 text-fg">
                        {preview.data?.hashStrong ? (
                          <ShieldCheck className="size-4 text-accent" />
                        ) : (
                          <Tooltip
                            content={
                              preview.data?.hashAlgorithm === "sha1"
                                ? "Base game jar uses SHA-1 integrity verification; loader libraries are verified with SHA-512."
                                : undefined
                            }
                          >
                            <span className="flex items-center">
                              <ShieldAlert className="size-4 text-warning" />
                            </span>
                          </Tooltip>
                        )}
                        {preview.data?.hashAlgorithm ? `Checked with ${preview.data.hashAlgorithm.toUpperCase()}` : "No checksum available"}
                        {preview.data?.hashAlgorithm === "sha1" && preview.data.notes.some((n) => n.includes("SHA-512")) && (
                          <span className="text-xs text-muted">(libraries SHA-512)</span>
                        )}
                        {preview.data?.downloadBytes ? ` · ${formatBytes(preview.data.downloadBytes)}` : ""}
                      </dd>
                    </dl>
                    {preview.data?.notes.map((n) => (
                      <p key={n} className="text-xs text-muted">
                        {n}
                      </p>
                    ))}
                    <div className="rounded-lg border border-border bg-surface-2 p-3">
                      <label className="flex items-start gap-2.5 text-[13px] text-fg">
                        <Checkbox checked={eula} onCheckedChange={(c) => setEula(c === true)} className="mt-0.5" />
                        <span>
                          I have read and accept the{" "}
                          <button
                            type="button"
                            className="inline-flex items-center gap-0.5 text-accent hover:underline"
                            onClick={() => api.app.openExternal("https://aka.ms/MinecraftEULA").catch((e) => toast.error(errorMessage(e)))}
                          >
                            Minecraft End User License Agreement <ExternalLink className="size-3" />
                          </button>
                          . MCPanel records your acceptance in the server's eula.txt.
                        </span>
                      </label>
                    </div>
                  </Card>
                )}

                <div className="flex justify-between">
                  <Button variant="ghost" disabled={step === 0} onClick={() => setStep((s) => s - 1)}>
                    Back
                  </Button>
                  {step < STEPS.length - 1 ? (
                    <Button variant="primary" disabled={!canNext} onClick={() => setStep((s) => s + 1)}>
                      Continue
                    </Button>
                  ) : (
                    <Button variant="primary" disabled={!canNext || !javaId || !version} onClick={create}>
                      Create server
                    </Button>
                  )}
                </div>
              </>
            )}
          </div>
          {!jobId && (
            <Card className="sticky top-0 hidden xl:block">
              <CardHeader title="Summary" description="Updates as you go." />
              <dl className="grid grid-cols-[88px_1fr] gap-x-3 gap-y-2.5 p-4 text-xs">
                <dt className="text-muted">Name</dt>
                <dd className="truncate text-fg">{name || "—"}</dd>
                <dt className="text-muted">Folder</dt>
                <dd className="font-mono text-[11px] break-all text-fg">{location.data?.directory ?? "—"}</dd>
                <dt className="text-muted">Software</dt>
                <dd className="text-fg">{sw?.displayName ?? "—"}</dd>
                <dt className="text-muted">Version</dt>
                <dd className="text-fg">
                  {version ?? "—"}
                  {version && build !== "latest" ? ` · build ${build}` : ""}
                </dd>
                <dt className="text-muted">Java</dt>
                <dd className="text-fg">{selectedJava ? `Java ${selectedJava.major}` : "—"}</dd>
                <dt className="text-muted">Memory</dt>
                <dd className="text-fg tabular-nums">
                  {minMem} – {maxMem} MB
                </dd>
                <dt className="text-muted">Template</dt>
                <dd className="text-fg">{template?.name ?? "None"}</dd>
              </dl>
            </Card>
          )}
        </div>
      </PageBody>
    </>
  );
}
