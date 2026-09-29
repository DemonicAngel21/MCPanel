import { useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { Activity, Archive, Coffee, LayoutDashboard, LayoutTemplate, Plus, Search, Server, Settings } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { CommandPalette } from "@/components/command-palette";
import { NotificationInbox } from "@/components/notification-inbox";
import { JobToasts } from "@/components/job-toasts";
import { QuitDialog } from "@/components/quit-dialog";
import { Button } from "@/components/ui/button";
import { Kbd, StatusDot, Tooltip } from "@/components/ui/primitives";
import { startEventBridge } from "@/lib/events";
import { useServers, useSettings } from "@/lib/queries";
import { stateMeta } from "@/lib/server-state";
import { cn } from "@/lib/utils";
import { useUi } from "@/stores/ui";

function useThemeSync() {
  const { data } = useSettings();
  const pref = data?.theme ?? "system";
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const resolved = pref === "system" ? (mq.matches ? "dark" : "light") : pref;
      document.documentElement.dataset.theme = resolved;
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [pref]);
}

function RailLink({ to, icon, label, exact }: { to: string; icon: ReactNode; label: string; exact?: boolean }) {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const active = exact ? path === to : path === to || path.startsWith(`${to}/`);
  return (
    <Tooltip content={label} side="right">
      <Link
        to={to}
        className={cn(
          "flex size-10 items-center justify-center rounded-lg text-muted transition-colors duration-150 hover:bg-surface-3 hover:text-fg active:scale-95 [&_svg]:size-[18px]",
          active && "bg-accent-soft text-accent hover:bg-accent-soft hover:text-accent",
        )}
        aria-label={label}
      >
        {icon}
      </Link>
    </Tooltip>
  );
}

/** Fades a page in on navigation; server tabs animate inside the server layout. */
function RouteTransition() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const key = path.startsWith("/servers/") && path !== "/servers/new" ? path.split("/").slice(0, 3).join("/") : path;
  return (
    <div key={key} className="flex min-h-0 flex-1 animate-page-in flex-col">
      <Outlet />
    </div>
  );
}

function ServerList() {
  const { data: servers, isLoading } = useServers();
  const path = useRouterState({ select: (s) => s.location.pathname });
  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-border bg-surface">
      <div className="flex h-12 items-center justify-between border-b border-border px-3">
        <span className="text-xs font-semibold tracking-wide text-muted uppercase">Servers</span>
        <Tooltip content="New server">
          <Button asChild variant="ghost" size="icon-sm">
            <Link to="/servers/new" aria-label="New server">
              <Plus />
            </Link>
          </Button>
        </Tooltip>
      </div>
      <nav aria-label="Servers" className="flex-1 overflow-y-auto p-2">
        {isLoading && <p className="px-2 py-1 text-xs text-faint">Loading…</p>}
        {servers?.length === 0 && (
          <div className="px-2 py-3 text-xs text-muted">
            No servers yet.{" "}
            <Link to="/servers/new" className="text-accent hover:underline">
              Create one
            </Link>
          </div>
        )}
        {servers?.map((s) => {
          const meta = stateMeta(s.state);
          const active = path.startsWith(`/servers/${s.id}`);
          return (
            <Link
              key={s.id}
              to="/servers/$serverId"
              params={{ serverId: s.id }}
              className={cn(
                "group flex items-center gap-2.5 rounded-md px-2 py-1.5 text-[13px] text-muted transition-colors duration-150 hover:bg-surface-3 hover:text-fg",
                active && "bg-surface-3 text-fg",
              )}
            >
              <StatusDot tone={meta.tone} pulse={meta.pulse} />
              <span className="min-w-0 flex-1 truncate">{s.name}</span>
              <span className="shrink-0 text-[11px] text-faint">{s.software.gameVersion}</span>
            </Link>
          );
        })}
      </nav>
      <button
        type="button"
        onClick={() => useUi.getState().setPaletteOpen(true)}
        className="m-2 flex items-center gap-2 rounded-md border border-border bg-surface-2 px-2.5 py-1.5 text-xs text-faint hover:text-muted"
      >
        <Search className="size-3.5" />
        <span className="flex-1 text-left">Search or run a command</span>
        <Kbd>Ctrl K</Kbd>
      </button>
    </aside>
  );
}

export function AppShell() {
  const qc = useQueryClient();
  useThemeSync();

  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    void startEventBridge(qc).then((fn) => {
      if (cancelled) fn();
      else stop = fn;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [qc]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        useUi.getState().setPaletteOpen(!useUi.getState().paletteOpen);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full">
      <nav aria-label="Main" className="flex w-14 shrink-0 flex-col items-center gap-1 border-r border-border bg-background py-3">
        <Link to="/" className="mb-3 flex size-9 items-center justify-center" aria-label="MCPanel home">
          <img src="/logo.svg" alt="" className="size-8" />
        </Link>
        <RailLink to="/" exact icon={<LayoutDashboard />} label="Dashboard" />
        <RailLink to="/servers" icon={<Server />} label="Servers" />
        <RailLink to="/backups" icon={<Archive />} label="Backups" />
        <RailLink to="/java" icon={<Coffee />} label="Java runtimes" />
        <RailLink to="/templates" icon={<LayoutTemplate />} label="Templates" />
        <RailLink to="/activity" icon={<Activity />} label="Activity" />
        <div className="flex-1" />
        <NotificationInbox />
        <RailLink to="/settings" icon={<Settings />} label="Settings" />
      </nav>
      <ServerList />
      <main className="flex min-w-0 flex-1 flex-col overflow-hidden bg-background">
        <RouteTransition />
      </main>
      <CommandPalette />
      <QuitDialog />
      <JobToasts />
    </div>
  );
}

export function PageHeader({
  title,
  description,
  actions,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="shrink-0 border-b border-border bg-surface px-6 pt-4">
      <div className="flex items-start justify-between gap-4 pb-4">
        <div className="min-w-0">
          <h1 className="truncate text-lg font-semibold text-fg">{title}</h1>
          {description && <div className="mt-0.5 text-xs text-muted">{description}</div>}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
      </div>
      {children}
    </header>
  );
}

export function PageBody({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("min-h-0 flex-1 overflow-y-auto px-6 py-5", className)}>{children}</div>;
}
