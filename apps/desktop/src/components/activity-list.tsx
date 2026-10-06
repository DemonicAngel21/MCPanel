import { Bot, CheckCircle2, History, XCircle } from "lucide-react";
import type { AuditEntryDto } from "@/bindings/AuditEntryDto";
import { formatDateTime, formatRelative } from "@/lib/format";
import { EmptyState, Tooltip } from "./ui/primitives";

const ACTIONS: Record<string, string> = {
  "ai.configure": "Configured AI Assistant",
  "ai.command": "Executed server command",
  "ai.action": "Executed AI task",
  "ai.task": "Completed AI task",
  "server.create": "Created server",
  "server.import": "Imported server",
  "server.update": "Changed server settings",
  "server.delete": "Removed server",
  "server.start": "Started server",
  "server.stop": "Stopped server",
  "server.force_stop": "Force-stopped server",
  "server.restart": "Restarted server",
  "server.restart_policy": "Changed the restart policy",
  "server.template_applied": "Applied a template",
  "content.install": "Installed a plugin/mod",
  "content.remove": "Removed a plugin/mod",
  "content.disable": "Disabled a plugin/mod",
  "content.enable": "Enabled a plugin/mod",
  "content.discard_pending": "Discarded a queued plugin/mod change",
  "bedrock.enable": "Set up Bedrock crossplay",
  "server.diagnostics_export": "Exported diagnostics",
  "encryption.setup": "Set up backup encryption",
  "encryption.import": "Imported a Recovery Kit",
  "cloud.disconnect": "Disconnected cloud storage",
  "playit.link_start": "Started linking MCPanel's playit agent",
  "playit.unlink": "Unlinked MCPanel's playit agent",
  "playit.start": "Started MCPanel's playit agent",
  "playit.stop": "Stopped MCPanel's playit agent",
  "playit.tunnel_create": "Created a playit tunnel",
  "playit.tunnel_rename": "Renamed a playit tunnel",
  "playit.tunnel_port": "Changed a playit tunnel's port",
  "playit.tunnel_enable": "Enabled a playit tunnel",
  "playit.tunnel_disable": "Disabled a playit tunnel",
  "playit.tunnel_delete": "Deleted a playit tunnel",
  "tunnel.agent_start": "Started the playit service",
  "tunnel.agent_stop": "Stopped the playit service",
  "tunnel.link_start": "Started linking the playit agent",
  "tunnel.address_set": "Changed the public address",
  "bedrock.configure": "Changed the Bedrock settings",
  "player.op": "Made a player operator",
  "player.deop": "Removed an operator",
  "player.whitelist_add": "Added a player to the whitelist",
  "player.whitelist_remove": "Removed a player from the whitelist",
  "player.set_whitelist": "Turned the whitelist on/off",
  "player.ban": "Banned a player",
  "player.pardon": "Unbanned a player",
  "player.ban_ip": "Banned an IP address",
  "player.pardon_ip": "Unbanned an IP address",
  "player.kick": "Kicked a player",
  "backup.create": "Created a backup",
  "backup.delete": "Deleted a backup",
  "backup.verify": "Verified a backup",
  "backup.restore": "Restored a backup",
  "backup.policy": "Changed the backup schedule",
  "backup.set_location": "Changed the backups folder",
  "server.crashed": "Server crashed",
  "server.failed": "Server failed to run",
  "server.eula_accepted": "Accepted the Minecraft EULA",
  "server.properties.update": "Changed server.properties",
  "file.write": "Edited file",
  "file.create": "Created file",
  "file.create_dir": "Created folder",
  "file.rename": "Renamed",
  "file.move": "Moved files",
  "file.copy": "Copied files",
  "file.delete": "Deleted files",
  "file.zip": "Created archive",
  "file.unzip": "Extracted archive",
  "file.import": "Uploaded",
  "file.export": "Downloaded file",
};

function detail(e: AuditEntryDto): string | null {
  const m = e.metadata as Record<string, unknown> | null;
  if (!m || typeof m !== "object") return e.target;
  if (typeof m.path === "string") return m.path;
  if (Array.isArray(m.paths)) return (m.paths as string[]).join(", ");
  if (Array.isArray(m.keys)) return (m.keys as string[]).join(", ");
  if (typeof m.name === "string") return m.name;
  if (typeof m.error === "string") return `Error: ${m.error}`;
  if (typeof m.diagnosis === "string") return m.diagnosis;
  return e.target;
}

export function ActivityList({ entries, serverNames }: { entries: AuditEntryDto[] | undefined; serverNames?: Record<string, string> }) {
  if (!entries) return null;
  if (entries.length === 0)
    return <EmptyState icon={<History />} title="No activity yet" description="Actions such as starting servers or editing files appear here." />;
  return (
    <ul className="divide-y divide-border">
      {entries.map((e) => {
        const raw = detail(e);
        const isServerIdentity = !!e.serverId && (raw === e.serverId || (serverNames ? raw === serverNames[e.serverId] : false));
        const isPlayitIdentity = (e.action.startsWith("tunnel.") || e.action.startsWith("playit.")) && raw === "playit";
        const d = raw && !isServerIdentity && !isPlayitIdentity ? raw : null;
        return (
          <li key={e.id} className="flex items-start gap-3 px-4 py-2.5">
            {e.result === "success" ? (
              <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-accent" />
            ) : (
              <XCircle className="mt-0.5 size-4 shrink-0 text-danger" />
            )}
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-[13px] text-fg">{ACTIONS[e.action] ?? e.action}</span>
                {e.actor === "ai" && (
                  <span className="inline-flex items-center gap-1 rounded border border-accent/30 bg-accent-soft px-1.5 py-0.5 text-[10px] font-medium text-accent">
                    <Bot className="size-3" />
                    AI
                  </span>
                )}
                {serverNames && e.serverId && serverNames[e.serverId] && <span className="text-[13px] text-muted"> · {serverNames[e.serverId]}</span>}
              </div>
              {d && <p className="selectable truncate font-mono text-[11px] text-muted">{d}</p>}
            </div>
            <Tooltip content={formatDateTime(e.occurredAt)}>
              <span className="shrink-0 text-[11px] text-faint">{formatRelative(e.occurredAt)}</span>
            </Tooltip>
          </li>
        );
      })}
    </ul>
  );
}
