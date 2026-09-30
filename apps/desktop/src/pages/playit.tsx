import { Link } from "@tanstack/react-router";
import { Copy, ExternalLink, Globe } from "lucide-react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import { PlayitAgentCard, ServerAddressEditor } from "@/components/playit-card";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, EmptyState, StatusDot } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { useServers, useTunnel } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

const open = (url: string) => api.app.openExternal(url).catch((e) => toast.error(errorMessage(e)));

/** Internet access through playit.gg: the agent, and each server's public address. */
export function PlayitPage() {
  const { data: servers } = useServers();
  const { data: t } = useTunnel();
  const ready = t?.agentRunning === true && t.secretConfigured === true;
  return (
    <>
      <PageHeader
        title="Playit.gg"
        description="Let friends outside your network join your servers through free playit.gg tunnels, without opening router ports."
        actions={
          <Button variant="outline" onClick={() => open("https://playit.gg/account/tunnels")}>
            Tunnels on playit.gg <ExternalLink />
          </Button>
        }
      />
      <PageBody className="max-w-5xl space-y-5">
        <PlayitAgentCard />

        <Card>
          <CardHeader
            title="Servers"
            description="Each server needs its own tunnel. On playit.gg, add a Minecraft Java tunnel for this agent to the server's local address, then save the tunnel's public address here."
          />
          {servers && servers.length === 0 && (
            <EmptyState icon={<Globe />} title="No servers yet" description="Create a server first, then give it a tunnel here." />
          )}
          <ul className="divide-y divide-border">
            {servers?.map((s) => {
              const port = s.port ?? 25565;
              const local = `127.0.0.1:${port}`;
              const live = s.state === "running" || s.state === "starting";
              return (
                <li key={s.id} className="space-y-3 px-4 py-4">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <Link
                      to="/servers/$serverId"
                      params={{ serverId: s.id }}
                      className="flex items-center gap-2 text-[13px] font-medium text-fg hover:underline"
                    >
                      <StatusDot tone={live ? "success" : "neutral"} /> {s.name}
                    </Link>
                    <span className="flex items-center gap-1 text-xs text-muted">
                      Local address <code className="selectable font-mono text-fg">{local}</code>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Copy local address of ${s.name}`}
                        onClick={() => navigator.clipboard.writeText(local).then(() => toast.success("Local address copied"))}
                      >
                        <Copy />
                      </Button>
                    </span>
                  </div>
                  <ServerAddressEditor serverId={s.id} port={port} ready={ready} steps={false} />
                </li>
              );
            })}
          </ul>
        </Card>

        <Card>
          <CardHeader title="Creating and changing tunnels" />
          <div className="space-y-2 p-4 text-xs text-muted">
            <p>
              Tunnels are created, edited and deleted in the playit.gg dashboard. Players connect to the tunnel's public address (for example{" "}
              <code className="font-mono">name.joinmc.link</code>); playit forwards them to this computer while the agent runs.
            </p>
            <p>Bedrock players (with Geyser) need a separate Minecraft Bedrock tunnel to the server's Bedrock port.</p>
          </div>
        </Card>
      </PageBody>
    </>
  );
}
