import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Coffee, FilePlus2, MoreHorizontal, RefreshCw, ScanSearch, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/overlays";
import { Badge, Card, EmptyState, Spinner } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatRelative } from "@/lib/format";
import { qk, useJava } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

export function JavaPage() {
  const qc = useQueryClient();
  const { data: runtimes, isLoading } = useJava();
  const refresh = () => qc.invalidateQueries({ queryKey: qk.java });
  const detect = useMutation({
    mutationFn: api.java.detect,
    onSuccess: (list) => {
      void refresh();
      toast.success(`Found ${list.filter((j) => j.valid).length} usable Java runtime(s)`);
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
  const add = async () => {
    try {
      const grant = await api.dialog.pickJava();
      if (!grant) return;
      const rt = await api.java.add(grant.token);
      void refresh();
      toast.success(`Added Java ${rt.major} (${rt.version})`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      void refresh();
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  return (
    <>
      <PageHeader
        title="Java runtimes"
        description="Each server can use a different Java version. MCPanel verifies every runtime by running it."
        actions={
          <>
            <Button variant="outline" onClick={add}>
              <FilePlus2 /> Add manually
            </Button>
            <Button variant="primary" onClick={() => detect.mutate()} disabled={detect.isPending}>
              {detect.isPending ? <Spinner className="text-accent-fg" /> : <ScanSearch />} Detect
            </Button>
          </>
        }
      />
      <PageBody>
        <Card>
          {isLoading ? (
            <p className="p-4 text-xs text-muted">Loading…</p>
          ) : runtimes?.length === 0 ? (
            <EmptyState
              icon={<Coffee />}
              title="No Java runtimes found"
              description="Install a 64-bit Java runtime (for example Eclipse Temurin 21 or 25), then press Detect — or add a java.exe manually."
            />
          ) : (
            <table className="w-full text-[13px]">
              <thead>
                <tr className="border-b border-border text-left text-xs text-muted">
                  <th className="px-4 py-2 font-medium">Version</th>
                  <th className="px-4 py-2 font-medium">Vendor</th>
                  <th className="px-4 py-2 font-medium">Location</th>
                  <th className="px-4 py-2 font-medium">Status</th>
                  <th className="px-4 py-2" />
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                {runtimes?.map((j) => (
                  <tr key={j.id} className="hover:bg-surface-2">
                    <td className="px-4 py-2.5">
                      <span className="font-semibold text-fg">Java {j.major || "?"}</span>
                      <span className="ml-2 text-xs text-muted">{j.version}</span>
                    </td>
                    <td className="px-4 py-2.5 text-muted">{j.vendor ?? "—"}</td>
                    <td className="max-w-md px-4 py-2.5">
                      <p className="selectable truncate font-mono text-[11px] text-muted" title={j.path}>
                        {j.path}
                      </p>
                      <p className="text-[11px] text-faint">
                        {j.source === "manual" ? "Added manually" : "Detected"} · checked {formatRelative(j.validatedAt)}
                      </p>
                    </td>
                    <td className="px-4 py-2.5">
                      {j.valid ? (
                        <Badge tone="success">Ready · {j.is64bit ? "64-bit" : "32-bit"}</Badge>
                      ) : (
                        <Badge tone="danger" title={j.validationError ?? undefined}>
                          {j.validationError ?? "Invalid"}
                        </Badge>
                      )}
                    </td>
                    <td className="px-4 py-2.5 text-right">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label="Actions">
                            <MoreHorizontal />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent>
                          <DropdownMenuItem onSelect={() => act(() => api.java.revalidate(j.id))}>
                            <RefreshCw /> Check again
                          </DropdownMenuItem>
                          <DropdownMenuItem destructive onSelect={() => act(() => api.java.remove(j.id))}>
                            <Trash2 /> Remove from MCPanel
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>
        <p className="mt-3 text-xs text-faint">
          Removing a runtime only forgets it in MCPanel; nothing is uninstalled. Servers using it must be switched to another runtime before they can
          start.
        </p>
      </PageBody>
    </>
  );
}
