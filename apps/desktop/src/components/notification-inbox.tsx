import * as PopoverPrimitive from "@radix-ui/react-popover";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { AlertTriangle, Bell, CheckCheck, Info, Trash2, UserPlus } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import type { NotificationDto } from "@/bindings/NotificationDto";
import { Button } from "@/components/ui/button";
import { EmptyState, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatRelative } from "@/lib/format";
import { qk, useNotifications, useUnreadNotifications } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";

function Icon({ n }: { n: NotificationDto }) {
  if (n.category === "player_joined") return <UserPlus className="size-4 text-info" />;
  if (n.severity === "error" || n.severity === "warning")
    return <AlertTriangle className={cn("size-4", n.severity === "error" ? "text-danger" : "text-warning")} />;
  return <Info className="size-4 text-muted" />;
}

/** Bell in the global rail with the unread count; opens the notification inbox. */
export function NotificationInbox() {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const { data: unread = 0 } = useUnreadNotifications();
  const { data: items } = useNotifications(open);
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: qk.notifications });
    void qc.invalidateQueries({ queryKey: qk.unreadNotifications });
  };
  const run = (p: Promise<void>) => p.then(refresh).catch((e) => toast.error(errorMessage(e)));
  const openItem = (n: NotificationDto) => {
    if (!n.read) void run(api.notifications.markRead([n.id]));
    if (n.serverId) {
      setOpen(false);
      void navigate({ to: "/servers/$serverId", params: { serverId: n.serverId } });
    }
  };
  return (
    <PopoverPrimitive.Root open={open} onOpenChange={setOpen}>
      <Tooltip content="Notifications" side="right">
        <PopoverPrimitive.Trigger asChild>
          <button
            type="button"
            aria-label={unread > 0 ? `Notifications (${unread} unread)` : "Notifications"}
            className={cn(
              "relative flex size-10 items-center justify-center rounded-lg text-muted transition-colors hover:bg-surface-3 hover:text-fg [&_svg]:size-[18px]",
              open && "bg-surface-3 text-fg",
            )}
          >
            <Bell />
            {unread > 0 && (
              <span className="absolute top-1.5 right-1.5 flex min-w-4 items-center justify-center rounded-full bg-danger px-1 text-[10px] leading-4 font-semibold text-white">
                {unread > 99 ? "99+" : unread}
              </span>
            )}
          </button>
        </PopoverPrimitive.Trigger>
      </Tooltip>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          side="right"
          align="end"
          sideOffset={8}
          className="z-50 flex max-h-[70vh] w-96 animate-fade-in flex-col rounded-lg border border-border-strong bg-surface-2 shadow-xl"
        >
          <div className="flex items-center justify-between border-b border-border px-3 py-2">
            <span className="text-[13px] font-semibold text-fg">Notifications</span>
            <div className="flex gap-1">
              <Button size="sm" variant="ghost" disabled={unread === 0} onClick={() => void run(api.notifications.markRead(null))}>
                <CheckCheck /> Mark all read
              </Button>
              <Tooltip content="Clear all">
                <Button
                  size="icon-sm"
                  variant="ghost"
                  aria-label="Clear all notifications"
                  disabled={!items?.length}
                  onClick={() => void run(api.notifications.clear())}
                >
                  <Trash2 />
                </Button>
              </Tooltip>
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">
            {items?.length === 0 && (
              <EmptyState icon={<Bell />} title="No notifications" description="Crashes and failed backups or tasks appear here." />
            )}
            {items?.map((n) => (
              <button
                key={n.id}
                type="button"
                onClick={() => openItem(n)}
                className={cn(
                  "flex w-full items-start gap-2.5 border-b border-border px-3 py-2.5 text-left last:border-b-0 hover:bg-surface-3",
                  !n.read && "bg-accent-soft/40",
                )}
              >
                <span className="mt-0.5 shrink-0">
                  <Icon n={n} />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-baseline justify-between gap-2">
                    <span className={cn("truncate text-[13px]", n.read ? "text-muted" : "font-medium text-fg")}>{n.title}</span>
                    <span className="shrink-0 text-[11px] text-faint">{formatRelative(n.createdAt)}</span>
                  </span>
                  {n.body && <span className="mt-0.5 line-clamp-3 block text-xs text-muted">{n.body}</span>}
                </span>
              </button>
            ))}
          </div>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}
