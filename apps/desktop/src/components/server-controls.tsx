import { MoreHorizontal, Play, RotateCw, Skull, Square } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import type { ServerDto } from "@/bindings/ServerDto";
import { api } from "@/lib/api";
import { canStart, canStop } from "@/lib/server-state";
import { errorMessage } from "@/lib/utils";
import { Button } from "./ui/button";
import { ConfirmDialog, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./ui/overlays";
import { Tooltip } from "./ui/primitives";

export function ServerControls({ server, compact }: { server: ServerDto; compact?: boolean }) {
  const [busy, setBusy] = useState(false);
  const [confirmKill, setConfirmKill] = useState(false);
  const s = server.state;
  const act = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const processAlive = ["starting", "running", "stopping", "restarting", "detached"].includes(s);

  return (
    <div className="flex items-center gap-2">
      {canStart(s) && (
        <Button variant="primary" size={compact ? "sm" : "md"} disabled={busy} onClick={() => act(() => api.servers.start(server.id))}>
          <Play /> Start
        </Button>
      )}
      {canStop(s) && (
        <>
          <Button
            variant="outline"
            size={compact ? "sm" : "md"}
            disabled={busy}
            className="transition-colors hover:border-danger hover:bg-danger hover:text-white [&:hover_svg]:text-white"
            onClick={() => act(() => api.servers.stop(server.id))}
          >
            <Square /> Stop
          </Button>
          {!compact && (
            <Button variant="outline" disabled={busy} onClick={() => act(() => api.servers.restart(server.id))}>
              <RotateCw /> Restart
            </Button>
          )}
        </>
      )}
      {(s === "stopping" || s === "restarting") && (
        <Tooltip
          content={
            s === "stopping" ? "Stopping gracefully (waits up to 60s before terminating if unresponsive, or use Force stop)" : "Restarting server"
          }
        >
          <span>
            <Button variant="outline" size={compact ? "sm" : "md"} disabled>
              <Square /> {s === "stopping" ? "Stopping…" : "Restarting…"}
            </Button>
          </span>
        </Tooltip>
      )}
      {processAlive && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size={compact ? "icon-sm" : "icon"} aria-label="More actions">
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            <DropdownMenuItem destructive onSelect={() => setConfirmKill(true)}>
              <Skull /> Force stop
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      <ConfirmDialog
        open={confirmKill}
        onOpenChange={setConfirmKill}
        title={`Force stop ${server.name}?`}
        description="The server process is terminated immediately. World changes since the last save may be lost."
        confirmLabel="Force stop"
        destructive
        onConfirm={() => act(() => api.servers.stop(server.id, true))}
      />
    </div>
  );
}
