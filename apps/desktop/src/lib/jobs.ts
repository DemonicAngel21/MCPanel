import type { JobDto } from "@/bindings/JobDto";
import { api } from "./api";

/** Poll a job until it finishes (succeeded, failed or cancelled). */
export async function waitForJob(id: string, intervalMs = 500): Promise<JobDto> {
  for (;;) {
    const job = await api.jobs.get(id);
    if (job.status !== "running" && job.status !== "queued") return job;
    await new Promise((r) => setTimeout(r, intervalMs));
  }
}
