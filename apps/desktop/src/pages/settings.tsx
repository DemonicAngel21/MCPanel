import { useQueryClient } from "@tanstack/react-query";
import { FolderOpen, Shield } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Card, CardHeader, Field, Input } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, useAppInfo, useSettings } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

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

function NumberSetting({ value, onSave }: { value: number; onSave: (n: number) => void }) {
  const [text, setText] = useState(String(value));
  return (
    <div className="flex items-center gap-2">
      <Input className="w-24" inputMode="numeric" value={text} onChange={(e) => setText(e.target.value.replace(/\D/g, ""))} />
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
                />
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
                />
              )}
            </Row>
          </div>
        </Card>

        <Card>
          <CardHeader title="Privacy" />
          <div className="flex items-start gap-3 px-4 py-3 text-xs text-muted">
            <Shield className="mt-0.5 size-4 shrink-0 text-accent" />
            <p>
              MCPanel has <span className="text-fg">no telemetry</span>. It contacts the internet only to download server software and version
              information from the official providers (Mojang, PaperMC, PurpurMC). It does not open any network port.
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
            <p className="text-xs text-faint">Licensed under MIT OR Apache-2.0. Not affiliated with Mojang Studios or Microsoft.</p>
          </div>
        </Card>
      </PageBody>
    </>
  );
}
