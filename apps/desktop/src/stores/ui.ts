import { create } from "zustand";
import type { GrantDto } from "@/bindings/GrantDto";

export type ActiveJob = {
  id: string;
  kind: string;
  serverId: string | null;
  status: string;
  progress: number | null;
  message: string | null;
};

type UiState = {
  paletteOpen: boolean;
  setPaletteOpen: (open: boolean) => void;
  quitRequest: number | null;
  setQuitRequest: (running: number | null) => void;
  jobs: Record<string, ActiveJob>;
  upsertJob: (job: ActiveJob) => void;
  droppedFiles: GrantDto[] | null;
  setDroppedFiles: (grants: GrantDto[] | null) => void;
};

export const useUi = create<UiState>((set) => ({
  paletteOpen: false,
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  quitRequest: null,
  setQuitRequest: (quitRequest) => set({ quitRequest }),
  jobs: {},
  upsertJob: (job) =>
    set((s) => ({
      jobs: { ...s.jobs, [job.id]: { ...s.jobs[job.id], ...job, message: job.message ?? s.jobs[job.id]?.message ?? null } },
    })),
  droppedFiles: null,
  setDroppedFiles: (droppedFiles) => set({ droppedFiles }),
}));
