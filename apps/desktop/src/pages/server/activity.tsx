import { PageBody } from "@/app/app-shell";
import { ActivityList } from "@/components/activity-list";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/primitives";
import { useAuditPages } from "@/lib/queries";
import { useServerId } from "./use-server-id";

export function ServerActivity() {
  const id = useServerId();
  const q = useAuditPages(id);
  return (
    <PageBody>
      <Card>
        <ActivityList entries={q.data?.pages.flat()} />
        {q.hasNextPage && (
          <div className="border-t border-border p-3 text-center">
            <Button variant="ghost" size="sm" onClick={() => q.fetchNextPage()} disabled={q.isFetchingNextPage}>
              Load older entries
            </Button>
          </div>
        )}
      </Card>
    </PageBody>
  );
}
