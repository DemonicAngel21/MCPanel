import { describe, expect, it } from "vitest";
import { formatBytes, formatDuration, formatPercent } from "./format";
import { parseMinecraftText, stripFormatting } from "./minecraft-text";
import { validateProperty } from "./properties";
import { canStart, canStop, hasProcess, stateMeta } from "./server-state";

describe("minecraft text", () => {
  it("returns plain text untouched", () => {
    expect(parseMinecraftText("hello")).toEqual([{ text: "hello" }]);
  });

  it("parses section-sign colour and format codes", () => {
    const s = parseMinecraftText("§aGreen §lbold§r plain");
    expect(s[0]).toMatchObject({ text: "Green ", color: "#51cf66" });
    expect(s[1]).toMatchObject({ text: "bold", bold: true, color: "#51cf66" });
    expect(s[2]).toEqual({ text: " plain" });
  });

  it("parses ANSI SGR sequences and strips them", () => {
    const s = parseMinecraftText("\u001b[31mred\u001b[0m ok");
    expect(s[0]).toMatchObject({ text: "red", color: "#ff6b6b" });
    expect(stripFormatting("\u001b[31mred\u001b[0m §aok")).toBe("red ok");
  });

  it("never produces HTML (text only)", () => {
    const s = parseMinecraftText("§c<script>alert(1)</script>");
    expect(s.map((x) => x.text).join("")).toBe("<script>alert(1)</script>");
  });
});

describe("format", () => {
  it("formats bytes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.0 GB");
    expect(formatBytes(null)).toBe("—");
  });

  it("formats durations", () => {
    expect(formatDuration(5_000)).toBe("5s");
    expect(formatDuration(65_000)).toBe("1m 5s");
    expect(formatDuration(3_700_000)).toBe("1h 1m");
    expect(formatDuration(90_000_000)).toBe("1d 1h");
  });

  it("formats percentages", () => {
    expect(formatPercent(4.25)).toBe("4.3%");
    expect(formatPercent(42)).toBe("42%");
    expect(formatPercent(undefined)).toBe("—");
  });
});

describe("server state helpers", () => {
  it("maps actions to states", () => {
    expect(canStart("stopped")).toBe(true);
    expect(canStart("running")).toBe(false);
    expect(canStop("running")).toBe(true);
    expect(canStop("detached")).toBe(false);
    expect(hasProcess("detached")).toBe(true);
    expect(stateMeta("crashed").tone).toBe("danger");
    expect(stateMeta("weird").label).toBe("weird");
  });
});

describe("property validation hints", () => {
  const schema = {
    kind: "integer",
    values: [],
    suggestions: [],
    min: 2,
    max: 32,
    default: "10",
    category: "performance",
    label: "View distance",
    description: null,
    sensitive: false,
  };
  it("checks integer ranges", () => {
    expect(validateProperty(schema, "12")).toBeNull();
    expect(validateProperty(schema, "1")).toBe("Minimum is 2");
    expect(validateProperty(schema, "40")).toBe("Maximum is 32");
    expect(validateProperty(schema, "abc")).toBe("Must be a whole number");
    expect(validateProperty(null, "anything")).toBeNull();
  });
});
