import { useQueryClient } from "@tanstack/react-query";
import { FolderOpen, Shield } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import { CloudCard } from "@/components/cloud-card";
import { PlayitPanel } from "@/components/playit-card";
import { EncryptionCard } from "@/components/encryption-card";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Card, CardHeader, Checkbox, Field, Input, Switch } from "@/components/ui/primitives";
import type { NotificationPrefsDto } from "@/bindings/NotificationPrefsDto";
import { api } from "@/lib/api";
import { qk, useAppInfo, useNotificationPrefs, useSettings } from "@/lib/queries";
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
  return (
    <div className="flex items-center gap-2">
      <Input className="w-24" inputMode="numeric" aria-label={label} value={text} onChange={(e) => setText(e.target.value.replace(/\D/g, ""))} />
      <Button size="sm" onClick={() => onSave(Number(text))} disabled={Number(text) === value || text === ""}>
        Save
      </Button>
    </div>
  );
}

export function SettingsPage() {
  const qc = useQueryClient();
  const { data: settings } = useSettings();
  const { data: info } = useAppInfo();

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
      <PageBody className="max-w-3xl space-y-5">
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
        </Card>

        <Card>
          <CardHeader title="Behaviour" />
          <div className="divide-y divide-border">
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

        <CloudCard />
        <Card>
          <CardHeader title="Internet access" description="Let friends outside your network join through a playit.gg tunnel." />
          <div className="p-4">
            <PlayitPanel />
          </div>
        </Card>

        <NotificationRules />

        <Card>
          <CardHeader title="Privacy" />
          <div className="flex items-start gap-3 px-4 py-3 text-xs text-muted">
            <Shield className="mt-0.5 size-4 shrink-0 text-accent" />
            <p>
              MCPanel has <span className="text-fg">no telemetry</span>. It contacts the internet only for what you ask it to do: server software and
              version information from the official providers (Mojang, PaperMC, PurpurMC, FabricMC, QuiltMC, NeoForged, MinecraftForge), plugins and
              mods from Modrinth, Hangar, GeyserMC and SpigotMC (through the Spiget API), and player profiles from Mojang. It does not open any
              network port.
            </p>
          </div>
        </Card>

        <Card>
          <CardHeader title="About" />
          <div className="space-y-3 px-4 py-3">
            <Field label="Version">
              <p className="text-[13px] text-fg">MCPanel {info?.version}</p>
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
