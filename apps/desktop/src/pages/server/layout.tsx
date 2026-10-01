import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { AlertTriangle, ExternalLink, FolderOpen, ServerOff, Unplug } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import type { ServerDto } from "@/bindings/ServerDto";
import { ServerControls } from "@/components/server-controls";
import { SoftwareMark } from "@/components/software-mark";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/overlays";
import { Badge, Banner, EmptyState, Spinner } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { useServer, useSoftware } from "@/lib/queries";
import { stateMeta } from "@/lib/server-state";
import { cn, errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

const TABS = [
  { to: "", label: "Manage" },
  { to: "/overview", label: "Overview" },
  { to: "/console", label: "Console" },
  { to: "/files", label: "Files" },
  { to: "/players", label: "Players" },
  { to: "/content", label: "Plugins" },
  { to: "/bedrock", label: "Bedrock" },
  { to: "/properties", label: "Properties" },
  { to: "/backups", label: "Backups" },
  { to: "/settings", label: "Settings" },
  { to: "/activity", label: "Activity" },
] as const;

function ServerBanners({ server }: { server: ServerDto }) {
  const [killOpen, setKillOpen] = useState(false);
  return (
    <div className="space-y-2 px-3 pt-4 empty:hidden sm:px-4 lg:px-6">
      {!server.directoryExists && (
        <Banner tone="danger" icon={<AlertTriangle />} title="Server folder not found">
          <span className="selectable font-mono">{server.directory}</span> does not exist or is not accessible.
        </Banner>
      )}
      {!server.eulaAccepted && server.directoryExists && (
        <Banner
          tone="warning"
          icon={<AlertTriangle />}
          title="The Minecraft EULA has not been accepted for this server"
          actions={
            <>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => api.app.openExternal("https://aka.ms/MinecraftEULA").catch((e) => toast.error(errorMessage(e)))}
              >
                Read EULA <ExternalLink />
              </Button>
              <Button size="sm" variant="primary" onClick={() => api.servers.acceptEula(server.id).catch((e) => toast.error(errorMessage(e)))}>
                I accept
              </Button>
            </>
          }
        >
          Minecraft servers only start after you accept Mojang's End User License Agreement.
        </Banner>
      )}
      {server.state === "detached" && (
        <Banner
          tone="warning"
          icon={<Unplug />}
          title="This server kept running while MCPanel was closed"
          actions={
            <Button size="sm" variant="danger-outline" onClick={() => setKillOpen(true)}>
              Force stop
            </Button>
          }
        >
          Its console is not connected, so it cannot be stopped gracefully from MCPanel. You can wait for it to exit (for example by running
          <span className="font-mono"> /stop</span> in-game) or force-stop it.
        </Banner>
      )}
      {server.diagnosis && (server.state === "error" || server.state === "crashed") && (
        <Banner tone="danger" icon={<AlertTriangle />} title={server.state === "error" ? "The server could not run" : "The server crashed"}>
          {server.diagnosis.message}
          {server.lastExitCode != null && ` (exit code ${server.lastExitCode})`}
        </Banner>
      )}
      <ConfirmDialog
        open={killOpen}
        onOpenChange={setKillOpen}
        title="Force stop detached server?"
        description="The process tree is terminated immediately. Unsaved world changes may be lost."
        destructive
        confirmLabel="Force stop"
        onConfirm={async () => {
          try {
            await api.servers.stop(server.id, true);
          } catch (e) {
            toast.error(errorMessage(e));
          }
        }}
      />
    </div>
  );
}

export function ServerLayout() {
  const id = useServerId();
  const { data: server, isLoading, error } = useServer(id);
  const { data: software } = useSoftware();
  const path = useRouterState({ select: (s) => s.location.pathname });
  const base = `/servers/${id}`;

  if (isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <Spinner />
      </div>
    );
  }
  if (!server) return <EmptyState icon={<ServerOff />} title="Server not found" description={error ? errorMessage(error) : undefined} />;
  const meta = stateMeta(server.state);
  // The content tab is named after (and only shown for) what the software supports.
  const content = software?.find((sw) => sw.id === server.software.softwareId)?.content ?? [];
  const contentLabel = content.some((c) => c.endsWith("_plugins")) ? "Plugins" : content.some((c) => c.endsWith("_mods")) ? "Mods" : null;
  const tabs: { to: string; label: string }[] = TABS.flatMap((t): { to: string; label: string }[] =>
    t.to === "/content" ? (contentLabel ? [{ to: t.to, label: contentLabel }] : []) : t.to === "/bedrock" ? (contentLabel ? [t] : []) : [t],
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="workspace-header shrink-0 border-b border-border px-3 pt-4 sm:px-4 lg:px-6">
        <div className="flex items-start justify-between gap-4 pb-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2.5">
              <SoftwareMark softwareId={server.software.softwareId} name={server.software.softwareName} size="sm" />
              <h1 className="truncate text-lg font-semibold text-fg">{server.name}</h1>
              <Badge tone={meta.tone}>{meta.label}</Badge>
            </div>
            <p className="mt-0.5 text-xs text-muted">
              {server.software.softwareName} {server.software.gameVersion}
              {server.software.build && ` · build ${server.software.build}`}
              {server.port && ` · port ${server.port}`}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="icon"
              aria-label="Open folder"
              title="Open server folder"
              onClick={() => api.servers.openFolder(id).catch((e) => toast.error(errorMessage(e)))}
            >
              <FolderOpen />
            </Button>
            <ServerControls server={server} />
          </div>
        </div>
        <nav
          aria-label="Server sections"
          className="-mb-px flex [scrollbar-width:none] gap-1 overflow-x-auto [mask-image:linear-gradient(to_right,black_calc(100%-24px),transparent)] pr-6"
        >
          {tabs.map((t) => {
            const to = `${base}${t.to}`;
            const active =
              t.to === "" ? path === base || path === `${base}/` : path.startsWith(to) || (t.to === "/files" && path.startsWith(`${base}/edit`));
            return (
              <Link
                key={t.label}
                to={to}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "shrink-0 border-b-2 px-3 py-2 text-[13px] whitespace-nowrap transition-colors duration-150",
                  active ? "border-accent font-medium text-fg" : "border-transparent text-muted hover:border-border-strong hover:text-fg",
                )}
              >
                {t.label}
              </Link>
            );
          })}
        </nav>
      </header>
      <ServerBanners server={server} />
      <div key={path.slice(base.length).split("/")[1] ?? ""} className="flex min-h-0 flex-1 animate-page-in flex-col">
        <Outlet />
      </div>
    </div>
  );
}
