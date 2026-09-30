/** Accent colors: presets (MCPanel green by default), the Windows accent or a custom color. */

export const ACCENT_PRESETS = [
  { id: "green", label: "Green", swatch: "#22c55e" },
  { id: "emerald", label: "Emerald", swatch: "#10b981" },
  { id: "teal", label: "Teal", swatch: "#14b8a6" },
  { id: "blue", label: "Blue", swatch: "#3b82f6" },
  { id: "violet", label: "Violet", swatch: "#8b5cf6" },
  { id: "rose", label: "Rose", swatch: "#f43f5e" },
  { id: "orange", label: "Orange", swatch: "#f97316" },
  { id: "amber", label: "Amber", swatch: "#f59e0b" },
] as const;

/** Tailwind shades 400/500/600/700/800/950 of each preset (green uses the stylesheet). */
const SHADES: Record<string, [string, string, string, string, string, string]> = {
  emerald: ["#34d399", "#10b981", "#059669", "#047857", "#065f46", "#022c22"],
  teal: ["#2dd4bf", "#14b8a6", "#0d9488", "#0f766e", "#115e59", "#042f2e"],
  blue: ["#60a5fa", "#3b82f6", "#2563eb", "#1d4ed8", "#1e40af", "#172554"],
  violet: ["#a78bfa", "#8b5cf6", "#7c3aed", "#6d28d9", "#5b21b6", "#2e1065"],
  rose: ["#fb7185", "#f43f5e", "#e11d48", "#be123c", "#9f1239", "#4c0519"],
  orange: ["#fb923c", "#f97316", "#ea580c", "#c2410c", "#9a3412", "#431407"],
  amber: ["#fbbf24", "#f59e0b", "#d97706", "#b45309", "#92400e", "#451a03"],
};

type Rgb = [number, number, number];

function hexToRgb(hex: string): Rgb {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function rgbToHex([r, g, b]: Rgb): string {
  return `#${[r, g, b]
    .map((v) =>
      Math.round(Math.min(255, Math.max(0, v)))
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

function luminance([r, g, b]: Rgb): number {
  const lin = (v: number) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(hexToRgb(a)), luminance(hexToRgb(b))].sort((p, q) => q - p) as [number, number];
  return (x + 0.05) / (y + 0.05);
}

function mix(a: Rgb, b: Rgb, t: number): Rgb {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
}

/** Move `hex` toward white (dark theme) or black (light theme) until it reads on `bg`. */
function readable(hex: string, bg: string, towards: Rgb, min: number): string {
  let c = hexToRgb(hex);
  for (let i = 0; i < 20 && contrast(rgbToHex(c), bg) < min; i++) c = mix(c, towards, 0.1);
  return rgbToHex(c);
}

export type AccentVars = Record<"--accent" | "--accent-strong" | "--accent-fg" | "--accent-text" | "--accent-soft" | "--ring", string>;

const rgba = (hex: string, a: number) => {
  const [r, g, b] = hexToRgb(hex);
  return `rgb(${r} ${g} ${b} / ${a})`;
};

/** CSS variables for an accent in a theme; `null` keeps the stylesheet's green. */
export function accentVars(accent: string, theme: "dark" | "light", systemColor: string | null): AccentVars | null {
  const dark = theme === "dark";
  const shades = SHADES[accent];
  if (shades) {
    const [s400, s500, s600, s700, s800, s950] = shades;
    return dark
      ? {
          "--accent": s400,
          "--accent-strong": s500,
          "--accent-fg": s950,
          "--accent-text": s400,
          "--accent-soft": rgba(s400, 0.12),
          "--ring": rgba(s400, 0.45),
        }
      : {
          "--accent": s700,
          "--accent-strong": s800,
          "--accent-fg": "#ffffff",
          "--accent-text": s800,
          "--accent-soft": rgba(s600, 0.1),
          "--ring": rgba(s700, 0.4),
        };
  }
  const base = accent === "system" ? systemColor : /^#[0-9a-f]{6}$/i.test(accent) ? accent : null;
  if (!base) return null;
  // Readable on the page background (dark #0b0f14 / light #ffffff) as text and as a fill.
  const main = dark ? readable(base, "#0b0f14", [255, 255, 255], 4.5) : readable(base, "#ffffff", [0, 0, 0], 4.5);
  const strong = rgbToHex(mix(hexToRgb(main), dark ? [0, 0, 0] : [0, 0, 0], 0.15));
  const fg = contrast(main, "#ffffff") >= contrast(main, "#0b0f14") ? "#ffffff" : "#0b0f14";
  return {
    "--accent": main,
    "--accent-strong": strong,
    "--accent-fg": fg,
    "--accent-text": main,
    "--accent-soft": rgba(main, dark ? 0.12 : 0.1),
    "--ring": rgba(main, 0.45),
  };
}

const KEYS: (keyof AccentVars)[] = ["--accent", "--accent-strong", "--accent-fg", "--accent-text", "--accent-soft", "--ring"];

export function applyAccent(vars: AccentVars | null) {
  const style = document.documentElement.style;
  for (const k of KEYS) {
    if (vars) style.setProperty(k, vars[k]);
    else style.removeProperty(k);
  }
}
