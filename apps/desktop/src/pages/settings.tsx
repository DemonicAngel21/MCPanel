import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowUpCircle, CheckCircle2, Coffee, Download, FolderOpen, HardDrive, Loader2, RefreshCw, Rocket, Shield, Sparkles } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import { AccentPicker } from "@/components/accent-picker";
import { AccountPanel } from "@/components/account-panel";
import { AiConfigCard } from "@/components/ai-config-card";
import { EncryptionCard } from "@/components/encryption-card";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Badge, Card, CardHeader, Checkbox, Field, Input, Switch } from "@/components/ui/primitives";
import type { NotificationPrefsDto } from "@/bindings/NotificationPrefsDto";
import { useUi } from "@/stores/ui";
import { api } from "@/lib/api";
import {
  qk,
  useAppInfo,
  useBackupLocation,
  useJava,
  useNotificationPrefs,
  useSettings,
  useSoftware,
  useUpdateCheck,
  useUpdateStatus,
} from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

const RULES: { key: keyof NotificationPrefsDto; label: string; description: string }[] = [
  { key: "crash", label: "A server crashes", description: "With what the restart policy did about it." },
  { key: "backupFailed", label: "A backup fails", description: "Manual and scheduled backups." },
  { key: "taskFailed", label: "Another task fails", description: "Installs, restores, server creation, Bedrock setup." },
  { key: "playerJoined", label: "A player joins", description: "Any server managed by MCPanel." },
  { key: "diskLow", label: "A drive is almost full", description: "Below 5 GB or 5 % free on a drive with servers or backups." },
];

function NotificationRules() {
  const qc = useQueryClient();
  const { data: prefs } = useNotificationPrefs();
  if (!prefs) return null;
  const set = async (key: keyof NotificationPrefsDto, channel: "inbox" | "desktop", on: boolean) => {
    try {
      const next = await api.notifications.updatePrefs({ ...prefs, [key]: { ...prefs[key], [channel]: on } });
      qc.setQueryData(qk.notificationPrefs, next);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };
  return (
    <Card>
      <CardHeader title="Notifications" description="Choose what goes to the inbox (bell icon) and what Windows shows as a desktop notification." />
      <table className="w-full text-[13px]">
        <thead>
          <tr className="border-b border-border text-left text-xs text-muted">
            <th className="px-4 py-2 font-normal">When</th>
            <th className="w-20 px-2 py-2 text-center font-normal">Inbox</th>
            <th className="w-20 px-2 py-2 text-center font-normal">Desktop</th>
          </tr>
        </thead>
        <tbody>
          {RULES.map((r) => (
            <tr key={r.key} className="border-b border-border last:border-b-0">
              <td className="px-4 py-2.5">
                <p className="text-fg">{r.label}</p>
                <p className="text-xs text-muted">{r.description}</p>
              </td>
              {(["inbox", "desktop"] as const).map((c) => (
                <td key={c} className="px-2 py-2.5 text-center">
                  <Checkbox aria-label={`${r.label}: ${c}`} checked={prefs[r.key][c]} onCheckedChange={(v) => void set(r.key, c, v === true)} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </Card>
  );
}

function Row({ label, description, children }: { label: string; description?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 px-4 py-3">
      <div>
        <p className="text-[13px] text-fg">{label}</p>
        {description && <p className="mt-0.5 max-w-lg text-xs text-muted">{description}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function NumberSetting({ value, onSave, label }: { value: number; onSave: (n: number) => void; label: string }) {
  const [text, setText] = useState(String(value));
  const [prev, setPrev] = useState(value);
  if (value !== prev) {
    setPrev(value);
    setText(String(value));
  }
  return (
    <div className="flex items-center gap-2">
      <Input className="w-24" inputMode="numeric" aria-label={label} value={text} onChange={(e) => setText(e.target.value.replace(/\D/g, ""))} />
      <Button size="sm" onClick={() => onSave(Number(text))} disabled={Number(text) === value || text === ""}>
        Save
      </Button>
    </div>
  );
}

function BackupsStorageCard() {
  const qc = useQueryClient();
  const { data: loc } = useBackupLocation();
  const [busy, setBusy] = useState(false);

  const chooseFolder = async () => {
    setBusy(true);
    try {
      const grant = await api.dialog.pickFolder("Choose backup storage folder");
      if (!grant) return;
      const next = await api.backups.setLocation(grant.token);
      qc.setQueryData(qk.backupLocation, next);
      toast.success("Backup storage folder updated");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const resetFolder = async () => {
    setBusy(true);
    try {
      const next = await api.backups.setLocation(null);
      qc.setQueryData(qk.backupLocation, next);
      toast.success("Reset to default backup storage folder");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card>
      <CardHeader
        title="Backup Storage Location"
        description="Choose where world and server backups are saved on your computer."
        actions={loc?.isDefault ? <Badge tone="neutral">Default location</Badge> : <Badge tone="success">Custom location</Badge>}
      />
      <div className="space-y-3 p-4 text-[13px]">
        <Field label="Current folder">
          <p className="selectable truncate font-mono text-xs text-muted">{loc?.directory}</p>
        </Field>
        <div className="flex flex-wrap items-center gap-2 pt-1">
          <Button size="sm" variant="outline" onClick={() => void api.backups.openFolder().catch((e) => toast.error(errorMessage(e)))}>
            <FolderOpen /> Open Folder
          </Button>
          <Button size="sm" variant="outline" onClick={() => void chooseFolder()} disabled={busy}>
            <HardDrive /> Change Folder…
          </Button>
          {!loc?.isDefault && (
            <Button size="sm" variant="ghost" onClick={() => void resetFolder()} disabled={busy}>
              Reset to default
            </Button>
          )}
        </div>
      </div>
    </Card>
  );
}

function JavaSummaryCard() {
  const qc = useQueryClient();
  const { data: javaList } = useJava();
  const [detecting, setDetecting] = useState(false);

  const validCount = (javaList ?? []).filter((j) => j.valid).length;

  const detect = async () => {
    setDetecting(true);
    try {
      const list = await api.java.detect();
      qc.setQueryData(qk.java, list);
      toast.success(`Found ${list.filter((j) => j.valid).length} Java runtimes`);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setDetecting(false);
    }
  };

  return (
    <Card>
      <CardHeader
        title="Java Runtimes"
        description="Manage the Java versions installed on your machine for Minecraft compatibility."
        actions={
          <Badge tone={validCount > 0 ? "success" : "warning"}>
            {validCount} usable runtime{validCount === 1 ? "" : "s"}
          </Badge>
        }
      />
      <div className="space-y-3 p-4 text-[13px]">
        <p className="text-xs text-muted">Modern Minecraft requires Java 21+. Each server selects its own appropriate Java runtime automatically.</p>
        <div className="flex items-center gap-2">
          <Button asChild size="sm" variant="outline">
            <Link to="/java">
              <Coffee className="size-3.5" /> Manage Runtimes
            </Link>
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void detect()} disabled={detecting}>
            <RefreshCw className={`size-3.5 ${detecting ? "animate-spin" : ""}`} /> Detect Runtimes
          </Button>
        </div>
      </div>
    </Card>
  );
}

function ApplicationUpdatesCard() {
  const qc = useQueryClient();
  const { data: settings } = useSettings();
  const { data: info } = useAppInfo();
  const [checking, setChecking] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [showNotes, setShowNotes] = useState(false);

  const { data: updateInfo } = useUpdateCheck();
  const { data: updateStatus } = useUpdateStatus();

  const isDownloading = updateStatus?.status === "downloading" || downloading;
  const isReady = updateStatus?.status === "ready";

  const handleCheck = async () => {
    setChecking(true);
    try {
      const res = await api.updates.check(true);
      await qc.invalidateQueries({ queryKey: qk.updates });
      if (res.hasUpdate) {
        toast.info(`New version ${res.latestVersion} available!`);
      } else {
        toast.success("MCPanel is already up to date!");
      }
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setChecking(false);
    }
  };

  const handleDownload = async () => {
    setDownloading(true);
    try {
      await api.updates.download();
      await qc.invalidateQueries({ queryKey: qk.updateStatus });
      toast.info("Downloading update installer in background…");
    } catch (e) {
      toast.error(errorMessage(e));
      setDownloading(false);
    }
  };

  const handleInstall = async () => {
    try {
      await api.updates.install();
      toast.success("Installer started. Please complete setup in installer.");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const update = async (patch: Parameters<typeof api.settings.update>[0]) => {
    try {
      await api.settings.update(patch);
      await qc.invalidateQueries({ queryKey: qk.settings });
      toast.success("Update settings saved");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const formatMb = (bytes: number | null | undefined) => {
    if (!bytes) return "0 MB";
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <Card>
      <CardHeader
        title="Application Updates"
        description="Keep MCPanel up to date with new features, bug fixes, and security patches."
        actions={
          updateInfo?.hasUpdate ? (
            <Badge tone="warning" className="gap-1">
              <ArrowUpCircle className="size-3" /> Update Available
            </Badge>
          ) : (
            <Badge tone="success" className="gap-1">
              <CheckCircle2 className="size-3" /> Up to date
            </Badge>
          )
        }
      />
      <div className="space-y-4 p-4 text-[13px]">
        {/* Version status header */}
        <div className="flex items-center justify-between rounded-lg border border-border bg-surface-2 p-3">
          <div>
            <div className="flex items-center gap-2">
              <span className="font-semibold text-fg">MCPanel {info?.version}</span>
              {updateInfo?.hasUpdate && <span className="text-xs text-accent">{updateInfo.latestVersion} available</span>}
            </div>
            <p className="mt-0.5 text-xs text-muted">
              {updateInfo?.publishedAt
                ? `Latest release published on ${new Date(updateInfo.publishedAt).toLocaleDateString()}`
                : "Official desktop build"}
            </p>
          </div>
          <Button size="sm" variant="outline" onClick={() => void handleCheck()} disabled={checking || isDownloading}>
            <RefreshCw className={`size-3.5 ${checking ? "animate-spin" : ""}`} />
            <span>{checking ? "Checking…" : "Check Now"}</span>
          </Button>
        </div>

        {/* Ready to Install banner */}
        {isReady && (
          <div className="rounded-lg border-2 border-accent/40 bg-accent-soft p-3.5 text-accent-text">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h4 className="font-semibold text-fg">Update Downloaded & Ready to Install</h4>
                <p className="mt-1 text-xs text-muted">
                  Version {updateStatus?.version || updateInfo?.latestVersion} has been downloaded and verified. Click below to launch the installer.
                </p>
              </div>
              <Button size="sm" variant="primary" onClick={() => void handleInstall()} className="shrink-0">
                <Sparkles className="size-3.5" /> Install & Restart
              </Button>
            </div>
          </div>
        )}

        {/* Downloading progress bar */}
        {isDownloading && !isReady && (
          <div className="space-y-2 rounded-lg border border-border bg-surface-2 p-3.5">
            <div className="flex items-center justify-between text-xs">
              <span className="flex items-center gap-2 font-medium text-fg">
                <Loader2 className="size-3.5 animate-spin text-accent" />
                Downloading update installer…
              </span>
              <span className="font-mono text-muted">
                {Math.round((updateStatus?.progress ?? 0) * 100)}% ({formatMb(updateStatus?.downloadedBytes)} / {formatMb(updateStatus?.totalBytes)})
              </span>
            </div>
            <div className="h-2 w-full overflow-hidden rounded-full bg-surface-3">
              <div
                className="h-full bg-accent transition-all duration-300"
                style={{ width: `${Math.round((updateStatus?.progress ?? 0) * 100)}%` }}
              />
            </div>
          </div>
        )}

        {/* Update Available notification & details */}
        {updateInfo?.hasUpdate && !isReady && !isDownloading && (
          <div className="space-y-3 rounded-lg border border-warning/30 bg-warning/5 p-3.5">
            <div className="flex items-start justify-between gap-3">
              <div>
                <div className="flex items-center gap-2">
                  <h4 className="font-semibold text-fg">{updateInfo.releaseName || `MCPanel ${updateInfo.latestVersion}`}</h4>
                  <Badge tone="warning">New Version</Badge>
                </div>
                {updateInfo.assetName && (
                  <p className="mt-0.5 font-mono text-[11px] text-muted">
                    {updateInfo.assetName} ({formatMb(updateInfo.assetSize)})
                  </p>
                )}
              </div>
              <Button size="sm" variant="primary" onClick={() => void handleDownload()} disabled={!updateInfo.downloadUrl}>
                <Download className="size-3.5" /> Download Update
              </Button>
            </div>

            {updateInfo.releaseNotes && (
              <div>
                <button type="button" onClick={() => setShowNotes(!showNotes)} className="text-xs text-accent hover:underline">
                  {showNotes ? "Hide Release Notes" : "View Release Notes"}
                </button>
                {showNotes && (
                  <div className="mt-2 max-h-40 overflow-y-auto rounded border border-border bg-surface-2 p-2.5 font-mono text-[11px] whitespace-pre-wrap text-fg">
                    {updateInfo.releaseNotes}
                  </div>
                )}
              </div>
            )}
          </div>
        )}

        {/* Preferences */}
        <div className="divide-y divide-border border-t border-border pt-1">
          <Row label="Automatically check for updates" description="Checks GitHub releases in the background when MCPanel starts.">
            <Switch
              checked={settings?.autoCheckUpdates ?? true}
              onCheckedChange={(v) => void update({ autoCheckUpdates: v })}
              aria-label="Automatically check for updates"
            />
          </Row>
          <Row label="Include pre-release builds" description="Receive early preview updates and beta versions as soon as they are published.">
            <Switch
              checked={settings?.checkPrereleaseUpdates ?? false}
              onCheckedChange={(v) => void update({ checkPrereleaseUpdates: v })}
              aria-label="Include pre-release builds"
            />
          </Row>
        </div>
      </div>
    </Card>
  );
}

export function SettingsPage() {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const { data: settings } = useSettings();
  const { data: info } = useAppInfo();
  const { data: software } = useSoftware();

  const update = async (patch: Parameters<typeof api.settings.update>[0]) => {
    try {
      await api.settings.update(patch);
      await qc.invalidateQueries({ queryKey: qk.settings });
      toast.success("Settings saved");
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <>
      <PageHeader title="Settings" description="Preferences for MCPanel itself. Server settings live on each server's page." />
      <PageBody className="columns-1 gap-5 xl:columns-2 [&>*]:mb-5 [&>*]:break-inside-avoid">
        <Card>
          <CardHeader title="Account" description="Your MCPanel account (Firebase). Optional: MCPanel works without one." />
          <div className="p-4">
            <AccountPanel />
          </div>
        </Card>

        <AiConfigCard />

        <Card>
          <CardHeader title="Appearance" />
          <Row label="Theme" description="Follow Windows, or always use dark or light.">
            <Select
              aria-label="Theme"
              className="w-40"
              value={settings?.theme}
              onValueChange={(theme) => update({ theme })}
              options={[
                { value: "system", label: "System" },
                { value: "dark", label: "Dark" },
                { value: "light", label: "Light" },
              ]}
            />
          </Row>
          <AccentPicker value={settings?.accent ?? "green"} onChange={(accent) => update({ accent })} />
        </Card>

        <BackupsStorageCard />

        <JavaSummaryCard />

        <Card>
          <CardHeader title="Behaviour" />
          <div className="divide-y divide-border">
            <Row
              label="Getting Started on startup"
              description="Show the welcome and setup wizard whenever MCPanel launches. You can also relaunch it at any time."
            >
              <div className="flex items-center gap-3">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    useUi.getState().setSessionOnboardingDismissed(false);
                    void update({ onboardingCompleted: false });
                    void navigate({ to: "/getting-started" });
                  }}
                >
                  <Rocket className="size-3.5" /> Launch setup
                </Button>
                {settings && (
                  <Switch
                    checked={!settings.onboardingCompleted}
                    onCheckedChange={(v) => {
                      if (v) {
                        useUi.getState().setSessionOnboardingDismissed(false);
                      }
                      void update({ onboardingCompleted: !v });
                    }}
                    aria-label="Show Getting Started on startup"
                  />
                )}
              </div>
            </Row>
            <Row
              label="Closing the window"
              description="Closing the MCPanel window keeps it running in the system tray so servers stay online. Quit from the tray icon."
            >
              <span className="text-xs text-muted">Minimise to tray</span>
            </Row>
            <Row
              label="Stop timeout when quitting"
              description="How long MCPanel waits for servers to stop gracefully before terminating them (10–600 seconds)."
            >
              {settings && (
                <NumberSetting
                  key={settings.quitStopTimeoutSecs}
                  value={settings.quitStopTimeoutSecs}
                  onSave={(n) => update({ quitStopTimeoutSecs: n })}
                  label="Stop timeout when quitting (seconds)"
                />
              )}
            </Row>
            <Row
              label="Collect TPS and MSPT"
              description="Every 15 seconds MCPanel asks running servers for their tick times (tick query, or tps/mspt on Paper). The replies are hidden from the console but appear in the server's own log file."
            >
              {settings && (
                <Switch checked={settings.tickSampling} onCheckedChange={(v) => void update({ tickSampling: v })} aria-label="Collect TPS and MSPT" />
              )}
            </Row>
            <Row
              label="Console buffer"
              description="Lines of console output kept in memory per server (1,000–200,000). Takes effect after restarting MCPanel."
            >
              {settings && (
                <NumberSetting
                  key={settings.consoleBufferLines}
                  value={settings.consoleBufferLines}
                  onSave={(n) => update({ consoleBufferLines: n })}
                  label="Console buffer (lines)"
                />
              )}
            </Row>
          </div>
        </Card>

        <EncryptionCard />

        <NotificationRules />

        <Card>
          <CardHeader title="Privacy" />
          <div className="flex items-start gap-3 px-4 py-3 text-xs text-muted">
            <Shield className="mt-0.5 size-4 shrink-0 text-accent" />
            <p>
              MCPanel has <span className="text-fg">no telemetry</span>. It contacts the internet only for what you ask it to do: server software and
              version information from the official providers (
              {software?.map((s) => s.displayName).join(", ") || "Mojang, PaperMC, PurpurMC, FabricMC, QuiltMC, NeoForged, MinecraftForge"}), plugins
              and mods from Modrinth, Hangar, GeyserMC and SpigotMC (through the Spiget API), and player profiles from Mojang. It does not open any
              network port.
            </p>
          </div>
        </Card>

        <ApplicationUpdatesCard />

        <Card>
          <CardHeader title="About" />
          <div className="space-y-3 px-4 py-3">
            <Field label="Version">
              <div className="flex items-center gap-2">
                <span className="text-[13px] font-semibold text-fg">MCPanel {info?.version}</span>
                <Badge tone="success">Desktop Release</Badge>
              </div>
            </Field>
            <Field label="Data folder">
              <p className="selectable font-mono text-xs text-muted">{info?.dataDir}</p>
            </Field>
            <Field label="Default servers folder">
              <p className="selectable font-mono text-xs text-muted">{info?.defaultServersDir}</p>
            </Field>
            <Field label="Logs">
              <div className="flex items-center gap-3">
                <p className="selectable font-mono text-xs text-muted">{info?.logsDir}</p>
                <Button size="sm" variant="outline" onClick={() => api.app.openLogsFolder().catch((e) => toast.error(errorMessage(e)))}>
                  <FolderOpen /> Open
                </Button>
              </div>
            </Field>
            <p className="text-xs text-faint">
              MCPanel by DemonicAngel21 · Copyright © 2026 DemonicAngel21 · MIT License. Not affiliated with Mojang Studios or Microsoft.
            </p>
          </div>
        </Card>
      </PageBody>
    </>
  );
}
