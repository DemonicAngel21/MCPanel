export type StartupSettings = { onboardingCompleted: boolean };
export type StartupView = "loading" | "error" | "setup" | "app";

export function startupView(isPending: boolean, isError: boolean, settings?: StartupSettings): StartupView {
  if (isPending) return "loading";
  if (isError || !settings) return "error";
  return settings.onboardingCompleted ? "app" : "setup";
}
