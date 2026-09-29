/**
 * Bridge backend domain events into the UI: cache invalidation, job progress, quit and
 * drag-and-drop notifications.
 */
import { listen } from "@tauri-apps/api/event";
import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { EventDto } from "@/bindings/EventDto";
import type { GrantDto } from "@/bindings/GrantDto";
import { qk } from "./queries";
import { useUi } from "@/stores/ui";

export async function startEventBridge(qc: QueryClient): Promise<() => void> {
  const unlisteners = await Promise.all([
    listen<EventDto>("mcpanel://event", ({ payload: e }) => handle(qc, e)),
    listen("mcpanel://resync", () => void qc.invalidateQueries()),
    listen<number>("mcpanel://quit-requested", ({ payload }) => useUi.getState().setQuitRequest(payload)),
    listen<string>("mcpanel://notice", ({ payload }) => toast.error(payload)),
    listen<GrantDto[]>("mcpanel://files-dropped", ({ payload }) => useUi.getState().setDroppedFiles(payload)),
  ]);
  return () => unlisteners.forEach((u) => u());
}

function handle(qc: QueryClient, e: EventDto) {
  switch (e.type) {
    case "serverCreated":
    case "serverDeleted":
      void qc.invalidateQueries({ queryKey: qk.servers });
      break;
    case "serverUpdated":
    case "serverStateChanged":
    case "serverReady":
    case "serverStopped":
    case "playerJoined":
    case "playerLeft":
      void qc.invalidateQueries({ queryKey: qk.servers, exact: true });
      void qc.invalidateQueries({ queryKey: qk.server(e.serverId), exact: true });
      if (e.type === "serverUpdated") void qc.invalidateQueries({ queryKey: qk.serverProperties(e.serverId) });
      void qc.invalidateQueries({ queryKey: qk.players(e.serverId) });
      if (e.type === "serverStateChanged") {
        void qc.invalidateQueries({ queryKey: qk.content(e.serverId) });
        void qc.invalidateQueries({ queryKey: qk.bedrock(e.serverId) });
      }
      break;
    case "crashRecorded":
      void qc.invalidateQueries({ queryKey: qk.crashes(e.serverId) });
      break;
    case "contentChanged":
      void qc.invalidateQueries({ queryKey: qk.content(e.serverId) });
      void qc.invalidateQueries({ queryKey: qk.bedrock(e.serverId) });
      break;
    case "notificationCreated":
    case "notificationsChanged":
      void qc.invalidateQueries({ queryKey: qk.notifications });
      break;
    case "cloudChanged":
      void qc.invalidateQueries({ queryKey: qk.cloud });
      break;
    case "bedrockChanged":
      void qc.invalidateQueries({ queryKey: qk.bedrock(e.serverId) });
      break;
    case "playersChanged":
      void qc.invalidateQueries({ queryKey: qk.players(e.serverId) });
      break;
    case "serverCrashed":
      void qc.invalidateQueries({ queryKey: qk.servers, exact: true });
      void qc.invalidateQueries({ queryKey: qk.server(e.serverId), exact: true });
      break;
    case "jobUpdated": {
      useUi.getState().upsertJob({
        id: e.jobId,
        kind: e.kind,
        serverId: e.serverId,
        status: e.status,
        progress: e.progress,
        message: e.message,
      });
      if (e.status !== "running") void qc.invalidateQueries({ queryKey: qk.jobs });
      break;
    }
    case "javaRuntimesChanged":
      void qc.invalidateQueries({ queryKey: qk.java });
      break;
    case "settingsChanged":
      void qc.invalidateQueries({ queryKey: qk.settings });
      break;
    case "auditRecorded":
      void qc.invalidateQueries({ queryKey: ["audit"] });
      break;
    case "backupsChanged":
      void qc.invalidateQueries({ queryKey: ["backups"] });
      break;
  }
}
