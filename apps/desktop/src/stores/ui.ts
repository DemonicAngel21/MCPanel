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

const AUTOSAVE_STORAGE_KEY = "mcpanel:editor_autosave";
const WORDWRAP_STORAGE_KEY = "mcpanel:editor_wordwrap";

function getInitialAutosave(): boolean {
  try {
    if (typeof window !== "undefined" && window.localStorage) {
      const stored = window.localStorage.getItem(AUTOSAVE_STORAGE_KEY);
      if (stored !== null) return stored === "true";
    }
  } catch {
    // ignore
  }
  return true; // Default to true so users don't have to manually enable it on every file
}

function getInitialWordWrap(): boolean {
  try {
    if (typeof window !== "undefined" && window.localStorage) {
      const stored = window.localStorage.getItem(WORDWRAP_STORAGE_KEY);
      if (stored !== null) return stored === "true";
    }
  } catch {
    // ignore
  }
  return true; // Default to word wrap on for comfortable reading
}

type UiState = {
  paletteOpen: boolean;
  setPaletteOpen: (open: boolean) => void;
  quitRequest: number | null;
  setQuitRequest: (running: number | null) => void;
  jobs: Record<string, ActiveJob>;
  upsertJob: (job: ActiveJob) => void;
  droppedFiles: GrantDto[] | null;
  setDroppedFiles: (grants: GrantDto[] | null) => void;
  editorAutosave: boolean;
  setEditorAutosave: (autosave: boolean) => void;
  editorWordWrap: boolean;
  setEditorWordWrap: (wordWrap: boolean) => void;
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
  editorAutosave: getInitialAutosave(),
  setEditorAutosave: (editorAutosave) => {
    try {
      if (typeof window !== "undefined" && window.localStorage) {
        window.localStorage.setItem(AUTOSAVE_STORAGE_KEY, String(editorAutosave));
      }
    } catch {
      // ignore
    }
    set({ editorAutosave });
  },
  editorWordWrap: getInitialWordWrap(),
  setEditorWordWrap: (editorWordWrap) => {
    try {
      if (typeof window !== "undefined" && window.localStorage) {
        window.localStorage.setItem(WORDWRAP_STORAGE_KEY, String(editorWordWrap));
      }
    } catch {
      // ignore
    }
    set({ editorWordWrap });
  },
}));
