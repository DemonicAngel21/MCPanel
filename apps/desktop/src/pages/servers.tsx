import { Link } from "@tanstack/react-router";
import { FolderInput, Plus, Server } from "lucide-react";
import { PageBody, PageHeader } from "@/app/app-shell";
import { ServerControls } from "@/components/server-controls";
import { Button } from "@/components/ui/button";
import { Badge, Card, EmptyState, SkeletonRows, StatusDot } from "@/components/ui/primitives";
import { formatRelative } from "@/lib/format";
import { useServers } from "@/lib/queries";
import { stateMeta } from "@/lib/server-state";

export function ServersPage() {
  const { data: servers, isLoading } = useServers();
  return (
    <>
      <PageHeader
        title="Servers"
        description="Every Minecraft server managed by MCPanel. Servers are ordinary folders you can also run without MCPanel."
        actions={
          <>
            <Button asChild variant="outline">
              <Link to="/servers/import">
                <FolderInput /> Import
              </Link>
            </Button>
            <Button asChild variant="primary">
              <Link to="/servers/new">
                <Plus /> New server
              </Link>
            </Button>
          </>
        }
      />
      <PageBody>
        <Card>
          {isLoading ? (
            <SkeletonRows rows={3} />
          ) : servers?.length === 0 ? (
            <EmptyState icon={<Server />} title="No servers yet" description="Create a server or import an existing server folder." />
          ) : (
            <table className="w-full text-[13px]">
              <thead>
                <tr className="border-b border-border text-left text-xs text-muted">
                  <th className="px-4 py-2 font-medium">Name</th>
                  <th className="px-4 py-2 font-medium">Software</th>
                  <th className="px-4 py-2 font-medium">Status</th>
                  <th className="px-4 py-2 font-medium">Port</th>
                  <th className="px-4 py-2 font-medium">Created</th>
                  <th className="px-4 py-2" />
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                {servers?.map((s) => {
                  const m = stateMeta(s.state);
                  return (
                    <tr key={s.id} className="table-row hover:bg-surface-2">
                      <td className="px-4 py-2.5">
                        <Link
                          to="/servers/$serverId"
                          params={{ serverId: s.id }}
                          className="flex items-center gap-2 font-medium text-fg hover:underline"
                        >
                          <StatusDot tone={m.tone} pulse={m.pulse} />
                          {s.name}
                        </Link>
                        <p className="selectable mt-0.5 truncate pl-4 text-[11px] text-faint">{s.directory}</p>
                      </td>
                      <td className="px-4 py-2.5 text-muted">
                        {s.software.softwareName} {s.software.gameVersion}
                        {s.software.build ? ` #${s.software.build}` : ""}
                      </td>
                      <td className="px-4 py-2.5">
                        <Badge tone={m.tone}>{m.label}</Badge>
                      </td>
                      <td className="px-4 py-2.5 text-muted tabular-nums">{s.port ?? "—"}</td>
                      <td className="px-4 py-2.5 text-muted">{formatRelative(s.createdAt)}</td>
                      <td className="px-4 py-2.5">
                        <div className="flex justify-end">
                          <ServerControls server={s} compact />
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </Card>
      </PageBody>
    </>
  );
}
