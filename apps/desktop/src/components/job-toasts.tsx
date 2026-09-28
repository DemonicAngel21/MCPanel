import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { useUi } from "@/stores/ui";

const LABELS: Record<string, string> = {
  "server.create": "Creating server",
  "content.install": "Installing",
  "backup.create": "Backing up",
  "backup.scheduled": "Scheduled backup",
  "backup.verify": "Verifying backup",
  "backup.restore": "Restoring backup",
};

/** Mirrors long-running jobs as toasts with live progress. */
export function JobToasts() {
  const jobs = useUi((s) => s.jobs);
  const shown = useRef(new Set<string>());

  useEffect(() => {
    for (const job of Object.values(jobs)) {
      const label = LABELS[job.kind] ?? "Working";
      const pct = job.progress != null ? ` ${Math.round(job.progress * 100)}%` : "";
      if (job.status === "running") {
        toast.loading(`${label}${pct}`, { id: job.id, description: job.message ?? undefined });
        shown.current.add(job.id);
      } else if (shown.current.has(job.id)) {
        shown.current.delete(job.id);
        if (job.status === "succeeded") toast.success(`${label}: done`, { id: job.id, description: undefined });
        else if (job.status === "cancelled") toast.message(`${label}: cancelled`, { id: job.id });
        else toast.error(`${label} failed`, { id: job.id, description: job.message ?? undefined, duration: 10_000 });
      }
    }
  }, [jobs]);

  return null;
}
