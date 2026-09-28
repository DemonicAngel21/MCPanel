import { useMemo, useState } from "react";
import { PageBody, PageHeader } from "@/app/app-shell";
import { ActivityList } from "@/components/activity-list";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Card } from "@/components/ui/primitives";
import { useAuditPages, useServers } from "@/lib/queries";

export function ActivityPage() {
  const { data: servers } = useServers();
  const [server, setServer] = useState<string>("all");
  const q = useAuditPages(server === "all" ? null : server);
  const names = useMemo(() => Object.fromEntries((servers ?? []).map((s) => [s.id, s.name])), [servers]);
  const entries = q.data?.pages.flat();
  return (
    <>
      <PageHeader
        title="Activity"
        description="Audit log of actions performed in MCPanel. Secrets are never recorded."
        actions={
          <Select
            className="w-56"
            value={server}
            onValueChange={setServer}
            options={[{ value: "all", label: "All servers" }, ...(servers ?? []).map((s) => ({ value: s.id, label: s.name }))]}
          />
        }
      />
      <PageBody>
        <Card>
          <ActivityList entries={entries} serverNames={names} />
          {q.hasNextPage && (
            <div className="border-t border-border p-3 text-center">
              <Button variant="ghost" size="sm" onClick={() => q.fetchNextPage()} disabled={q.isFetchingNextPage}>
                Load older entries
              </Button>
            </div>
          )}
        </Card>
      </PageBody>
    </>
  );
}
