import { describe, expect, it } from "vitest";
import { ACCENT_PRESETS, accentVars, contrast } from "./accent";

describe("accent colors", () => {
  it("keeps MCPanel green from the stylesheet", () => {
    expect(accentVars("green", "dark", null)).toBeNull();
    expect(accentVars("green", "light", null)).toBeNull();
  });

  it("gives every preset readable button text in both themes", () => {
    for (const p of ACCENT_PRESETS.filter((x) => x.id !== "green")) {
      for (const theme of ["dark", "light"] as const) {
        const v = accentVars(p.id, theme, null)!;
        expect(contrast(v["--accent"], v["--accent-fg"]), `${p.id} ${theme}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("makes custom and Windows colors readable on the page", () => {
    // A very dark navy and a pale yellow, the hardest cases.
    for (const c of ["#0a1a3a", "#fff59d"]) {
      const dark = accentVars(c, "dark", null)!;
      const light = accentVars(c, "light", null)!;
      expect(contrast(dark["--accent"], "#0b0f14")).toBeGreaterThanOrEqual(4.5);
      expect(contrast(light["--accent"], "#ffffff")).toBeGreaterThanOrEqual(4.5);
      expect(contrast(light["--accent"], light["--accent-fg"])).toBeGreaterThanOrEqual(4.5);
    }
    expect(accentVars("system", "dark", "#0078d7")).not.toBeNull();
    expect(accentVars("system", "dark", null)).toBeNull();
    expect(accentVars("not-a-color", "dark", null)).toBeNull();
  });
});
