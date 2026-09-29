import { useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, KeyRound, Radio, RefreshCw, Save, Smartphone } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import type { BedrockPieceDto } from "@/bindings/BedrockPieceDto";
import type { BedrockPongDto } from "@/bindings/BedrockPongDto";
import type { BedrockStatusDto } from "@/bindings/BedrockStatusDto";
import { PageBody } from "@/app/app-shell";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Badge, Banner, Card, CardHeader, Checkbox, EmptyState, Field, Input, Spinner } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { waitForJob } from "@/lib/jobs";
import { qk, useBedrock } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

const AUTH_OPTIONS = [
  { value: "floodgate", label: "Floodgate (Xbox account)", hint: "Bedrock players join without a Java account. Requires Floodgate." },
  { value: "online", label: "Java account", hint: "Bedrock players must also sign in with a Minecraft: Java Edition account." },
  { value: "offline", label: "Offline", hint: "Only for servers with online-mode=false." },
];

function Piece({ name, piece }: { name: string; piece: BedrockPieceDto | null }) {
  return (
    <div className="flex items-center justify-between gap-3 px-4 py-2.5 text-[13px]">
      <span className="font-medium text-fg">{name}</span>
      {piece ? (
        <span className="flex items-center gap-2 text-xs text-muted">
          <span className="selectable font-mono">{piece.version ?? piece.fileName}</span>
          {piece.pending ? (
            <Badge tone="warning">Applies after stop</Badge>
          ) : piece.enabled ? (
            <Badge tone="success">Installed</Badge>
          ) : (
            <Badge tone="neutral">Disabled</Badge>
          )}
        </span>
      ) : (
        <Badge tone="neutral">Not installed</Badge>
      )}
    </div>
  );
}

function SetupCard({ serverId, status }: { serverId: string; status: BedrockStatusDto }) {
  const [floodgate, setFloodgate] = useState(true);
  const [viaVersion, setViaVersion] = useState(status.viaVersionSuggested || status.viaVersionRequired);
  const [port, setPort] = useState(String(status.settings.port));
  const [busy, setBusy] = useState(false);
  const enable = async () => {
    setBusy(true);
    try {
      const job = await waitForJob(await api.bedrock.enable(serverId, { floodgate, viaVersion, port: Number(port) || null }));
      if (job.status === "succeeded") toast.success(status.running ? "Geyser is ready — restart the server to activate it" : "Geyser is ready — start the server");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card>
      <CardHeader title="Set up Bedrock crossplay" description="Installs Geyser so phones, consoles and Windows Bedrock players can join this Java server." />
      <div className="space-y-4 p-4">
        <label className="flex items-start gap-2 text-xs text-fg">
          <Checkbox checked={floodgate} onCheckedChange={(c) => setFloodgate(c === true)} className="mt-0.5" />
          <span>
            Also install <b>Floodgate</b> (recommended): Bedrock players join with their Xbox account and don't need to own Java Edition.
          </span>
        </label>
        {status.viaVersionAvailable && (
          <label className="flex items-start gap-2 text-xs text-fg">
            <Checkbox checked={viaVersion} onCheckedChange={(c) => setViaVersion(c === true)} className="mt-0.5" />
            <span>
              Install <b>ViaVersion</b>
              {status.viaVersionSuggested || status.viaVersionRequired
                ? " — needed: Geyser speaks a newer Minecraft version than this server runs."
                : " — only needed when the server runs an older Minecraft version than Geyser supports."}
            </span>
          </label>
        )}
        <Field label="Bedrock port (UDP)" hint={`Bedrock players connect to this port. The default is ${status.defaultPort}.`}>
          <Input inputMode="numeric" className="max-w-40" value={port} onChange={(e) => setPort(e.target.value.replace(/\D/g, "").slice(0, 5))} />
        </Field>
      </div>
      <div className="flex justify-end border-t border-border px-4 py-3">
        <Button variant="primary" onClick={enable} disabled={busy}>
          {busy ? <Spinner className="text-accent-fg" /> : <Smartphone />} Set up Geyser
        </Button>
      </div>
    </Card>
  );
}

function SettingsCard({ serverId, status }: { serverId: string; status: BedrockStatusDto }) {
  const qc = useQueryClient();
  const [port, setPort] = useState(String(status.settings.port));
  const [auth, setAuth] = useState(status.settings.authType);
  const [saving, setSaving] = useState(false);
  const save = async () => {
    setSaving(true);
    try {
      const next = await api.bedrock.configure(serverId, { port: Number(port) || 0, authType: auth });
      qc.setQueryData(qk.bedrock(serverId), next);
      toast.success(next.running ? "Saved — restart the server to apply" : "Bedrock settings saved");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };
  const options = AUTH_OPTIONS.map((o) => ({ ...o, disabled: o.value === "floodgate" && !status.floodgate }));
  return (
    <Card>
      <CardHeader
        title="Settings"
        description={
          status.configExists ? (
            <span>
              Stored in <span className="selectable font-mono">{status.configPath}</span>; other Geyser options stay as they are.
            </span>
          ) : (
            "Geyser creates its full configuration on first start and keeps these values."
          )
        }
      />
      <div className="grid grid-cols-2 gap-4 p-4">
        <Field label="Bedrock port (UDP)">
          <Input inputMode="numeric" value={port} onChange={(e) => setPort(e.target.value.replace(/\D/g, "").slice(0, 5))} />
        </Field>
        <Field label="Bedrock players sign in with" hint={options.find((o) => o.value === auth)?.hint}>
          <Select value={auth} onValueChange={setAuth} options={options} />
        </Field>
      </div>
      <div className="flex justify-end border-t border-border px-4 py-3">
        <Button variant="primary" onClick={save} disabled={saving}>
          {saving ? <Spinner className="text-accent-fg" /> : <Save />} Save
        </Button>
      </div>
    </Card>
  );
}

function ConnectionCard({ serverId, status }: { serverId: string; status: BedrockStatusDto }) {
  const [pong, setPong] = useState<BedrockPongDto | null>(null);
  const [testing, setTesting] = useState(false);
  const port = status.activePort ?? status.settings.port;
  const test = async () => {
    setTesting(true);
    try {
      setPong(await api.bedrock.ping(serverId));
    } catch (e) {
      setPong(null);
      toast.error(errorMessage(e));
    } finally {
      setTesting(false);
    }
  };
  return (
    <Card>
      <CardHeader
        title="Connecting"
        actions={
          <Button size="sm" variant="ghost" onClick={test} disabled={!status.running || testing}>
            {testing ? <Spinner /> : <Radio />} Test Bedrock connection
          </Button>
        }
      />
      <div className="space-y-3 p-4 text-xs text-muted">
        <p>
          In Minecraft Bedrock, open <b>Play → Servers → Add Server</b> and enter this computer's address with port{" "}
          <span className="selectable font-mono text-fg">{port}</span>. Players on the same network use its LAN IP address; Windows may ask to allow
          Java through the firewall for UDP the first time.
        </p>
        {pong && (
          <div className="flex items-center gap-2 rounded-md border border-accent/30 bg-accent/5 px-3 py-2 text-fg">
            <CheckCircle2 className="size-4 text-accent" />
            <span>
              Bedrock listener answered in {pong.latencyMs} ms: <b>{pong.motd}</b> · Bedrock {pong.version} · {pong.players ?? 0}/{pong.maxPlayers ?? "?"}{" "}
              players
            </span>
          </div>
        )}
        {!status.running && <p>Start the server to test the connection.</p>}
      </div>
    </Card>
  );
}

export function ServerBedrock() {
  const id = useServerId();
  const { data: status, isLoading, error, refetch, isFetching } = useBedrock(id);
  if (isLoading) {
    return (
      <PageBody className="flex items-center justify-center">
        <Spinner />
      </PageBody>
    );
  }
  if (!status) return <EmptyState title="Bedrock status unavailable" description={error ? errorMessage(error) : undefined} />;
  if (!status.supported) {
    return (
      <PageBody className="max-w-3xl">
        <EmptyState icon={<Smartphone />} title="Bedrock crossplay is not available for this server" description={status.unsupportedReason ?? undefined} />
      </PageBody>
    );
  }
  const installed = status.geyser != null;
  return (
    <PageBody className="max-w-3xl space-y-4">
      {status.restartRequired && (
        <Banner tone="warning" icon={<RefreshCw />} title="Restart the server to apply the Bedrock settings">
          Geyser is listening on port {status.activePort}; the saved port is {status.settings.port}.
        </Banner>
      )}
      {installed && (status.geyser?.pending || status.floodgate?.pending || status.viaVersion?.pending) && (
        <Banner tone="info" icon={<RefreshCw />} title="Restart the server to finish the setup">
          The new files are installed when the server stops or next starts.
        </Banner>
      )}
      {status.viaVersionRequired && (
        <Banner tone="warning" icon={<AlertTriangle />} title="Geyser needs ViaVersion on this server">
          Geyser reported that it targets a newer Minecraft version than this server runs. Install ViaVersion below or update the server.
        </Banner>
      )}
      {status.portInUse && (
        <Banner tone="danger" icon={<AlertTriangle />} title={`UDP port ${status.settings.port} is in use`}>
          {status.portInUseBy ? `${status.portInUseBy} is using it. ` : ""}The server will not start until the port is free or you choose another
          Bedrock port.
        </Banner>
      )}
      <Card>
        <CardHeader
          title="Bedrock crossplay"
          description="Geyser translates between Bedrock and Java Edition; Floodgate lets Bedrock players use their Xbox account."
          actions={
            <Button size="icon-sm" variant="ghost" aria-label="Refresh" onClick={() => void refetch()} disabled={isFetching}>
              <RefreshCw />
            </Button>
          }
        />
        <div className="divide-y divide-border">
          <Piece name="Geyser" piece={status.geyser} />
          <Piece name="Floodgate" piece={status.floodgate} />
          {status.viaVersionAvailable && <Piece name="ViaVersion" piece={status.viaVersion} />}
          {status.floodgate && (
            <div className="flex items-center gap-2 px-4 py-2.5 text-xs text-muted">
              <KeyRound className="size-3.5" />
              {status.floodgateKeyPresent
                ? "Floodgate key present. It is highly sensitive: MCPanel never shows or exports it, and backups containing it are marked Sensitive."
                : "Floodgate creates its key on first start."}
            </div>
          )}
        </div>
      </Card>
      {(!installed || (!status.floodgate && !status.running) || (status.viaVersionRequired && !status.viaVersion)) && (
        <SetupCard key={JSON.stringify([status.geyser, status.floodgate, status.viaVersion])} serverId={id} status={status} />
      )}
      {installed && <SettingsCard key={JSON.stringify(status.settings)} serverId={id} status={status} />}
      {installed && <ConnectionCard serverId={id} status={status} />}
    </PageBody>
  );
}
