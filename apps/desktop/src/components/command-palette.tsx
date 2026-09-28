import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Command } from "cmdk";
import {
  Activity,
  Archive,
  Coffee,
  LayoutTemplate,
  FolderInput,
  LayoutDashboard,
  Moon,
  Play,
  Plus,
  Server,
  Settings,
  Square,
  Sun,
} from "lucide-react";
import type { ReactNode } from "react";
import { toast } from "sonner";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { api } from "@/lib/api";
import { qk, useServers, useSettings } from "@/lib/queries";
import { canStart, canStop, stateMeta } from "@/lib/server-state";
import { errorMessage } from "@/lib/utils";
import { useUi } from "@/stores/ui";
import { StatusDot } from "./ui/primitives";

function Item({
  onSelect,
  icon,
  children,
  hint,
  value,
}: {
  onSelect: () => void;
  icon: ReactNode;
  children: ReactNode;
  hint?: ReactNode;
  value: string;
}) {
  return (
    <Command.Item
      value={value}
      onSelect={onSelect}
      className="flex cursor-default items-center gap-2.5 rounded-md px-2.5 py-2 text-[13px] text-fg data-[selected=true]:bg-surface-3 [&_svg]:size-4 [&_svg]:text-muted"
    >
      {icon}
      <span className="flex-1 truncate">{children}</span>
      {hint && <span className="text-xs text-faint">{hint}</span>}
    </Command.Item>
  );
}

function Group({ heading, children }: { heading: string; children: ReactNode }) {
  return (
    <Command.Group
      heading={heading}
      className="px-1 py-1 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-faint [&_[cmdk-group-heading]]:uppercase"
    >
      {children}
    </Command.Group>
  );
}

export function CommandPalette() {
  const open = useUi((s) => s.paletteOpen);
  const setOpen = useUi((s) => s.setPaletteOpen);
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { data: servers } = useServers();
  const { data: settings } = useSettings();

  const run = (fn: () => void | Promise<void>) => {
    setOpen(false);
    void Promise.resolve(fn()).catch((e) => toast.error(errorMessage(e)));
  };

  return (
    <DialogPrimitive.Root open={open} onOpenChange={setOpen}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-50 animate-fade-in bg-black/40" />
        <DialogPrimitive.Content className="fixed top-[18%] left-1/2 z-50 w-[min(600px,calc(100vw-32px))] -translate-x-1/2 animate-slide-up overflow-hidden rounded-xl border border-border-strong bg-surface shadow-2xl">
          <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
          <DialogPrimitive.Description className="sr-only">Search servers, pages and actions</DialogPrimitive.Description>
          <Command loop>
            <Command.Input
              autoFocus
              placeholder="Search servers, pages and actions…"
              className="h-12 w-full border-b border-border bg-transparent px-4 text-sm text-fg placeholder:text-faint focus:outline-none"
            />
            <Command.List className="max-h-96 overflow-y-auto p-1">
              <Command.Empty className="px-4 py-6 text-center text-xs text-muted">No results.</Command.Empty>
              {servers && servers.length > 0 && (
                <Group heading="Servers">
                  {servers.map((s) => {
                    const m = stateMeta(s.state);
                    return (
                      <Item
                        key={s.id}
                        value={`server ${s.name} ${s.software.softwareName} ${s.software.gameVersion}`}
                        icon={<StatusDot tone={m.tone} />}
                        hint={`${s.software.softwareName} ${s.software.gameVersion}`}
                        onSelect={() => run(() => navigate({ to: "/servers/$serverId", params: { serverId: s.id } }))}
                      >
                        {s.name}
                      </Item>
                    );
                  })}
                </Group>
              )}
              <Group heading="Actions">
                <Item value="create new server" icon={<Plus />} onSelect={() => run(() => navigate({ to: "/servers/new" }))}>
                  Create server
                </Item>
                <Item value="import existing server folder" icon={<FolderInput />} onSelect={() => run(() => navigate({ to: "/servers/import" }))}>
                  Import existing server
                </Item>
                {servers
                  ?.filter((s) => canStart(s.state))
                  .map((s) => (
                    <Item key={`start-${s.id}`} value={`start ${s.name}`} icon={<Play />} onSelect={() => run(() => api.servers.start(s.id))}>
                      Start {s.name}
                    </Item>
                  ))}
                {servers
                  ?.filter((s) => canStop(s.state))
                  .map((s) => (
                    <Item key={`stop-${s.id}`} value={`stop ${s.name}`} icon={<Square />} onSelect={() => run(() => api.servers.stop(s.id))}>
                      Stop {s.name}
                    </Item>
                  ))}
                <Item
                  value="detect java runtimes"
                  icon={<Coffee />}
                  onSelect={() =>
                    run(async () => {
                      const list = await api.java.detect();
                      void qc.invalidateQueries({ queryKey: qk.java });
                      toast.success(`${list.length} Java runtime${list.length === 1 ? "" : "s"} known`);
                    })
                  }
                >
                  Detect Java runtimes
                </Item>
                <Item
                  value="toggle theme dark light"
                  icon={settings?.theme === "light" ? <Moon /> : <Sun />}
                  onSelect={() =>
                    run(async () => {
                      await api.settings.update({ theme: document.documentElement.dataset.theme === "light" ? "dark" : "light" });
                      void qc.invalidateQueries({ queryKey: qk.settings });
                    })
                  }
                >
                  Toggle light/dark theme
                </Item>
              </Group>
              <Group heading="Pages">
                <Item value="dashboard" icon={<LayoutDashboard />} onSelect={() => run(() => navigate({ to: "/" }))}>
                  Dashboard
                </Item>
                <Item value="servers list" icon={<Server />} onSelect={() => run(() => navigate({ to: "/servers" }))}>
                  Servers
                </Item>
                <Item value="backups" icon={<Archive />} onSelect={() => run(() => navigate({ to: "/backups" }))}>
                  Backups
                </Item>
                <Item value="templates" icon={<LayoutTemplate />} onSelect={() => run(() => navigate({ to: "/templates" }))}>
                  Templates
                </Item>
                <Item value="java runtimes" icon={<Coffee />} onSelect={() => run(() => navigate({ to: "/java" }))}>
                  Java runtimes
                </Item>
                <Item value="activity audit log" icon={<Activity />} onSelect={() => run(() => navigate({ to: "/activity" }))}>
                  Activity
                </Item>
                <Item value="settings preferences" icon={<Settings />} onSelect={() => run(() => navigate({ to: "/settings" }))}>
                  Settings
                </Item>
              </Group>
            </Command.List>
          </Command>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
