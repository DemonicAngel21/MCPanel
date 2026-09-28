/** Presentation helpers for lifecycle states (the state machine itself lives in Rust). */

export type ServerState = "created" | "starting" | "running" | "stopping" | "stopped" | "crashed" | "restarting" | "error" | "detached";

export type Tone = "neutral" | "success" | "warning" | "danger" | "info";

const META: Record<ServerState, { label: string; tone: Tone; pulse?: boolean }> = {
  created: { label: "Not started", tone: "neutral" },
  starting: { label: "Starting", tone: "info", pulse: true },
  running: { label: "Running", tone: "success" },
  stopping: { label: "Stopping", tone: "warning", pulse: true },
  stopped: { label: "Stopped", tone: "neutral" },
  crashed: { label: "Crashed", tone: "danger" },
  restarting: { label: "Restarting", tone: "info", pulse: true },
  error: { label: "Error", tone: "danger" },
  detached: { label: "Running (detached)", tone: "warning" },
};

export function stateMeta(state: string) {
  return META[state as ServerState] ?? { label: state, tone: "neutral" as Tone };
}

export const canStart = (s: string) => s === "created" || s === "stopped" || s === "crashed" || s === "error";
export const hasProcess = (s: string) => ["starting", "running", "stopping", "restarting", "detached"].includes(s);
export const canStop = (s: string) => s === "starting" || s === "running";
