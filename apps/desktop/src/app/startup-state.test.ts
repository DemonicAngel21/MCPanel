import { describe, expect, it } from "vitest";
import { startupView } from "./startup-state";

describe("startup setup state", () => {
  it("waits for persisted settings before choosing a page", () => {
    expect(startupView(true, false)).toBe("loading");
  });

  it("opens setup when a fresh install has no completion flag", () => {
    expect(startupView(false, false, { onboardingCompleted: false })).toBe("setup");
  });

  it("opens the app after setup is completed", () => {
    expect(startupView(false, false, { onboardingCompleted: true })).toBe("app");
  });

  it("does not repeat setup after the user skips it", () => {
    expect(startupView(false, false, { onboardingCompleted: true })).toBe("app");
  });

  it("does not assume setup is complete if persisted settings fail to load", () => {
    expect(startupView(false, true)).toBe("error");
  });
});
