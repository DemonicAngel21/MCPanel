/**
 * Parse Minecraft `§` formatting codes and ANSI SGR escapes into styled segments.
 * Output is rendered as text nodes only — never as HTML.
 */

export type Segment = {
  text: string;
  color?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
};

// Minecraft's legacy palette, slightly adjusted for contrast on dark backgrounds.
const MC_COLORS: Record<string, string> = {
  "0": "#5c6370",
  "1": "#3b5bdb",
  "2": "#2f9e44",
  "3": "#1098ad",
  "4": "#e03131",
  "5": "#ae3ec9",
  "6": "#f08c00",
  "7": "#adb5bd",
  "8": "#6c757d",
  "9": "#5c7cfa",
  a: "#51cf66",
  b: "#3bc9db",
  c: "#ff6b6b",
  d: "#f783ac",
  e: "#fcc419",
  f: "#f1f3f5",
};

const ANSI_COLORS: Record<number, string> = {
  30: "#5c6370",
  31: "#ff6b6b",
  32: "#51cf66",
  33: "#fcc419",
  34: "#5c7cfa",
  35: "#f783ac",
  36: "#3bc9db",
  37: "#dee2e6",
  90: "#868e96",
  91: "#ff8787",
  92: "#8ce99a",
  93: "#ffe066",
  94: "#91a7ff",
  95: "#faa2c1",
  96: "#99e9f2",
  97: "#f8f9fa",
};

// eslint-disable-next-line no-control-regex
const TOKEN = /\u001b\[([0-9;]*)m|§([0-9a-fk-or])/gi;

export function parseMinecraftText(input: string): Segment[] {
  if (!input.includes("§") && !input.includes("\u001b")) return [{ text: input }];
  const out: Segment[] = [];
  let style: Omit<Segment, "text"> = {};
  let last = 0;
  for (const m of input.matchAll(TOKEN)) {
    const idx = m.index ?? 0;
    if (idx > last) out.push({ text: input.slice(last, idx), ...style });
    last = idx + m[0].length;
    if (m[2] !== undefined) {
      const c = m[2].toLowerCase();
      if (c in MC_COLORS) style = { color: MC_COLORS[c] };
      else if (c === "l") style = { ...style, bold: true };
      else if (c === "o") style = { ...style, italic: true };
      else if (c === "n") style = { ...style, underline: true };
      else if (c === "m") style = { ...style, strike: true };
      else if (c === "r") style = {};
    } else {
      const codes = (m[1] ?? "").split(";").filter(Boolean).map(Number);
      if (codes.length === 0) style = {};
      for (const code of codes) {
        if (code === 0) style = {};
        else if (code === 1) style = { ...style, bold: true };
        else if (code === 3) style = { ...style, italic: true };
        else if (code === 4) style = { ...style, underline: true };
        else if (code === 9) style = { ...style, strike: true };
        else if (code === 39) style = { ...style, color: undefined };
        else if (ANSI_COLORS[code]) style = { ...style, color: ANSI_COLORS[code] };
      }
    }
  }
  if (last < input.length) out.push({ text: input.slice(last), ...style });
  return out.filter((s) => s.text.length > 0);
}

export function stripFormatting(input: string): string {
  return input.replace(TOKEN, "");
}
