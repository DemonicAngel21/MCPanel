import { useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { errorMessage } from "@/lib/utils";
import { useUi } from "@/stores/ui";
import { Button } from "./ui/button";
import { Dialog, DialogContent } from "./ui/overlays";
import { Spinner } from "./ui/primitives";

/** Shown when the user quits from the tray while servers are running (decision #2). */
export function QuitDialog() {
  const running = useUi((s) => s.quitRequest);
  const setRunning = useUi((s) => s.setQuitRequest);
  const [busy, setBusy] = useState<"stop" | "leave" | null>(null);

  const quit = async (mode: "stop" | "leave") => {
    setBusy(mode);
    try {
      await api.app.quit(mode);
    } catch (e) {
      toast.error(errorMessage(e));
      setBusy(null);
    }
  };

  return (
    <Dialog open={running != null} onOpenChange={(o) => !o && !busy && setRunning(null)}>
      <DialogContent
        title="Quit MCPanel?"
        description={`${running ?? 0} server${running === 1 ? " is" : "s are"} still running.`}
        hideClose={busy != null}
        onEscapeKeyDown={(e) => busy && e.preventDefault()}
        footer={
          <>
            <Button variant="ghost" disabled={busy != null} onClick={() => setRunning(null)}>
              Cancel
            </Button>
            <Button variant="outline" disabled={busy != null} onClick={() => quit("leave")}>
              {busy === "leave" && <Spinner />} Quit and leave running
            </Button>
            <Button variant="primary" disabled={busy != null} onClick={() => quit("stop")} autoFocus>
              {busy === "stop" && <Spinner className="text-accent-fg" />} Stop servers and quit
            </Button>
          </>
        }
      >
        <div className="space-y-2 text-xs text-muted">
          <p>
            <span className="font-medium text-fg">Stop servers and quit</span> saves the worlds and shuts every server down gracefully before MCPanel
            exits. Servers that do not stop in time are terminated.
          </p>
          <p>
            <span className="font-medium text-fg">Quit and leave running</span> keeps the servers online without MCPanel. Their console will not be
            connected; the next time you open MCPanel they are shown as detached.
          </p>
          {busy === "stop" && <p className="text-fg">Stopping servers… this can take up to a minute.</p>}
        </div>
      </DialogContent>
    </Dialog>
  );
}
