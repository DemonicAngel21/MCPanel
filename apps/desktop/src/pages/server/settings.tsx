import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Save, Trash2 } from "lucide-react";
import { useState } from "react";
import type { ServerDto } from "@/bindings/ServerDto";
import { toast } from "sonner";
import { PageBody } from "@/app/app-shell";
import { Button } from "@/components/ui/button";
import { ConfirmDialog, Select } from "@/components/ui/overlays";
import { Banner, Card, CardHeader, Checkbox, Field, Input, Spinner, Textarea } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, useJava, useRestartPolicy, useServer, useSystemMetrics } from "@/lib/queries";
import type { RestartPolicyDto } from "@/bindings/RestartPolicyDto";
import { Switch } from "@/components/ui/primitives";
import { hasProcess } from "@/lib/server-state";
import { errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

const splitArgs = (s: string) =>
  s
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);

export function ServerSettings() {
  const id = useServerId();
  const { data: server } = useServer(id);
  if (!server) return null;
  // Re-initialise the form whenever the saved settings change.
  return <ServerSettingsForm key={`${server.id}:${server.name}:${JSON.stringify(server.launch)}`} server={server} />;
}

function ServerSettingsForm({ server }: { server: ServerDto }) {
  const id = server.id;
  const qc = useQueryClient();
  const navigate = useNavigate();
  const { data: java } = useJava();
  const { data: metrics } = useSystemMetrics();
  const [name, setName] = useState(server.name);
  const [javaId, setJavaId] = useState<string | undefined>(server.launch.javaRuntimeId ?? undefined);
  const [minMem, setMinMem] = useState(server.launch.minMemoryMb);
  const [maxMem, setMaxMem] = useState(server.launch.maxMemoryMb);
  const [jvm, setJvm] = useState(server.launch.jvmArgs.join("\n"));
  const [args, setArgs] = useState(server.launch.serverArgs.join("\n"));
  const [timeout, setTimeoutSecs] = useState(server.launch.stopTimeoutSecs);
  const [saving, setSaving] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteFiles, setDeleteFiles] = useState(false);
  const totalMb = metrics?.current ? Math.floor(metrics.current.memoryTotalBytes / 1024 / 1024) : null;
  const running = hasProcess(server.state);
  const selected = java?.find((j) => j.id === javaId);
  const javaTooOld = !!selected && server.software.javaMinMajor != null && selected.major < server.software.javaMinMajor;

  const save = async () => {
    setSaving(true);
    try {
      await api.servers.update(id, {
        name,
        launch: {
          javaRuntimeId: javaId ?? null,
          minMemoryMb: minMem,
          maxMemoryMb: maxMem,
          jvmArgs: splitArgs(jvm),
          serverArgs: splitArgs(args),
          stopTimeoutSecs: timeout,
        },
      });
      await qc.invalidateQueries({ queryKey: qk.servers });
      toast.success(running ? "Saved. Changes apply on the next start." : "Saved");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <PageBody className="max-w-3xl space-y-5">
      {running && (
        <Banner tone="info" title="The server is running">
          Launch settings apply the next time it starts.
        </Banner>
      )}
      <Card>
        <CardHeader title="General" />
        <div className="space-y-4 p-4">
          <Field label="Name">
            <Input value={name} onChange={(e) => setName(e.target.value)} maxLength={64} />
          </Field>
          <Field label="Folder" hint="MCPanel does not move server folders. To relocate a server, move the folder and import it again.">
            <p className="selectable font-mono text-xs text-muted">{server.directory}</p>
          </Field>
        </div>
      </Card>

      <Card>
        <CardHeader title="Java & memory" />
        <div className="grid grid-cols-2 gap-4 p-4">
          <Field
            label="Java runtime"
            className="col-span-2"
            error={javaTooOld ? `This server needs Java ${server.software.javaMinMajor} or newer.` : undefined}
            hint={server.software.javaMinMajor ? `Requires Java ${server.software.javaMinMajor}+.` : undefined}
          >
            <Select
              value={javaId}
              onValueChange={setJavaId}
              placeholder="Select a Java runtime"
              options={(java ?? []).map((j) => ({
                value: j.id,
                label: `Java ${j.major} · ${j.vendor ?? "unknown"}`,
                hint: j.valid ? j.path : "invalid",
                disabled: !j.valid,
              }))}
            />
          </Field>
          <Field label="Minimum memory (MB)">
            <Input inputMode="numeric" value={minMem} onChange={(e) => setMinMem(Number(e.target.value.replace(/\D/g, "")) || 0)} />
          </Field>
          <Field label="Maximum memory (MB)" hint={totalMb ? `This computer has ${totalMb} MB.` : undefined}>
            <Input inputMode="numeric" value={maxMem} onChange={(e) => setMaxMem(Number(e.target.value.replace(/\D/g, "")) || 0)} />
          </Field>
          <Field
            label="Additional JVM arguments"
            hint="One argument per line. Set memory with the fields above, not -Xmx/-Xms."
            className="col-span-2"
          >
            <Textarea rows={5} className="font-mono text-xs" value={jvm} onChange={(e) => setJvm(e.target.value)} placeholder="-XX:+UseG1GC" />
          </Field>
          <Field label="Additional server arguments" hint="One per line, passed after the jar (MCPanel always adds nogui)." className="col-span-2">
            <Textarea rows={2} className="font-mono text-xs" value={args} onChange={(e) => setArgs(e.target.value)} />
          </Field>
          <Field label="Graceful stop timeout (seconds)" hint="How long to wait for the server to save and stop before terminating it.">
            <Input inputMode="numeric" value={timeout} onChange={(e) => setTimeoutSecs(Number(e.target.value.replace(/\D/g, "")) || 0)} />
          </Field>
        </div>
        <div className="flex justify-end border-t border-border px-4 py-3">
          <Button variant="primary" onClick={save} disabled={saving || javaTooOld}>
            {saving ? <Spinner className="text-accent-fg" /> : <Save />} Save
          </Button>
        </div>
      </Card>

      <RestartPolicyCard serverId={id} />

      <Card className="border-danger/30">
        <CardHeader title="Danger zone" />
        <div className="flex items-center justify-between gap-4 p-4">
          <div>
            <p className="text-[13px] text-fg">Remove server from MCPanel</p>
            <p className="text-xs text-muted">By default the server folder and world stay on disk. Stop the server first.</p>
          </div>
          <Button variant="danger-outline" disabled={running} onClick={() => setDeleteOpen(true)}>
            <Trash2 /> Remove…
          </Button>
        </div>
      </Card>

      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={`Remove ${server.name}?`}
        description="MCPanel forgets this server and its console history."
        confirmLabel={deleteFiles ? "Remove and delete files" : "Remove"}
        destructive
        confirmText={deleteFiles ? server.name : undefined}
        onConfirm={async () => {
          try {
            await api.servers.delete(id, deleteFiles);
            await qc.invalidateQueries({ queryKey: qk.servers });
            toast.success(`${server.name} removed`);
            void navigate({ to: "/servers" });
          } catch (e) {
            toast.error(errorMessage(e));
          }
        }}
      >
        <label className="flex items-start gap-2 text-xs text-fg">
          <Checkbox checked={deleteFiles} onCheckedChange={(c) => setDeleteFiles(c === true)} className="mt-0.5" />
          <span>
            Also permanently delete the server folder, including worlds and player data. <span className="text-danger">This cannot be undone.</span>
          </span>
        </label>
      </ConfirmDialog>
    </PageBody>
  );
}

function RestartPolicyCard({ serverId }: { serverId: string }) {
  const { data } = useRestartPolicy(serverId);
  if (!data) return null;
  return <RestartPolicyForm key={JSON.stringify(data)} serverId={serverId} policy={data} />;
}

function RestartPolicyForm({ serverId, policy }: { serverId: string; policy: RestartPolicyDto }) {
  const qc = useQueryClient();
  const [p, setP] = useState(policy);
  const [saving, setSaving] = useState(false);
  const num = (v: string) => Number(v.replace(/\D/g, "")) || 0;
  const save = async () => {
    setSaving(true);
    try {
      qc.setQueryData(qk.restartPolicy(serverId), await api.crashes.updatePolicy(serverId, p));
      toast.success("Restart policy saved");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };
  return (
    <Card>
      <CardHeader
        title="Restart after a crash"
        description="Problems a restart cannot fix (wrong Java, EULA, port in use) are never restarted."
        actions={
          <label className="flex items-center gap-2 text-xs text-muted">
            {p.enabled ? "On" : "Off"}
            <Switch checked={p.enabled} onCheckedChange={(enabled) => setP({ ...p, enabled })} aria-label="Restart after a crash" />
          </label>
        }
      />
      <div className="grid grid-cols-2 gap-4 p-4">
        <Field label="Attempts" hint="How many restarts in a row before giving up.">
          <Input inputMode="numeric" value={p.maxAttempts} onChange={(e) => setP({ ...p, maxAttempts: num(e.target.value) })} />
        </Field>
        <Field label="Within (minutes)" hint="Crashes further apart start counting again.">
          <Input inputMode="numeric" value={Math.round(p.windowSecs / 60)} onChange={(e) => setP({ ...p, windowSecs: num(e.target.value) * 60 })} />
        </Field>
        <Field label="First delay (seconds)" hint="Doubled for each further attempt (at most 5 minutes).">
          <Input inputMode="numeric" value={p.delaySecs} onChange={(e) => setP({ ...p, delaySecs: num(e.target.value) })} />
        </Field>
        <Field label="Stable after (minutes)" hint="A server that ran this long starts counting from one again.">
          <Input inputMode="numeric" value={Math.round(p.stableSecs / 60)} onChange={(e) => setP({ ...p, stableSecs: num(e.target.value) * 60 })} />
        </Field>
        <label className="col-span-2 flex items-start gap-2 text-xs text-fg">
          <Checkbox checked={p.crashBackup} onCheckedChange={(c) => setP({ ...p, crashBackup: c === true })} className="mt-0.5" />
          <span>Back up the server before restarting it (keeps the state right after the crash)</span>
        </label>
      </div>
      <div className="flex justify-end border-t border-border px-4 py-3">
        <Button variant="primary" onClick={save} disabled={saving}>
          {saving ? <Spinner className="text-accent-fg" /> : <Save />} Save
        </Button>
      </div>
    </Card>
  );
}
