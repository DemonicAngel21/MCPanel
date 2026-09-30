import { Monitor } from "lucide-react";
import { useState } from "react";
import { Tooltip } from "@/components/ui/primitives";
import { ACCENT_PRESETS } from "@/lib/accent";
import { cn } from "@/lib/utils";

/** Accent presets, the Windows accent and a custom color. */
export function AccentPicker({ value, onChange }: { value: string; onChange: (accent: string) => void }) {
  const custom = value.startsWith("#");
  const [draft, setDraft] = useState(custom ? value : "#22c55e");
  const [touched, setTouched] = useState(false);
  return (
    <div className="space-y-3 border-t border-border px-4 py-3">
      <div>
        <p className="text-[13px] text-fg">Accent color</p>
        <p className="mt-0.5 text-xs text-muted">Used for buttons, highlights and charts. Green is MCPanel's own.</p>
      </div>
      <div role="radiogroup" aria-label="Accent color" className="flex flex-wrap items-center gap-2">
        {ACCENT_PRESETS.map((p) => (
          <Tooltip key={p.id} content={p.label}>
            <button
              type="button"
              role="radio"
              aria-checked={value === p.id}
              aria-label={p.label}
              onClick={() => onChange(p.id)}
              className={cn(
                "flex size-8 cursor-default items-center justify-center rounded-full border-2 transition-[scale,border-color] duration-150 hover:scale-110",
                value === p.id ? "border-fg" : "border-transparent",
              )}
            >
              <span className="size-5 rounded-full" style={{ background: p.swatch }} />
            </button>
          </Tooltip>
        ))}
        <span className="mx-1 h-6 w-px bg-border" aria-hidden />
        <button
          type="button"
          role="radio"
          aria-checked={value === "system"}
          onClick={() => onChange("system")}
          className={cn(
            "flex h-8 cursor-default items-center gap-1.5 rounded-full border px-3 text-xs transition-colors duration-150",
            value === "system" ? "border-accent bg-accent-soft text-fg" : "border-border text-muted hover:text-fg",
          )}
        >
          <Monitor className="size-3.5" /> Match Windows
        </button>
        <label
          className={cn(
            "flex h-8 items-center gap-1.5 rounded-full border px-2 text-xs transition-colors duration-150",
            custom ? "border-accent bg-accent-soft text-fg" : "border-border text-muted hover:text-fg",
          )}
        >
          <input
            type="color"
            aria-label="Custom accent color"
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              setTouched(true);
            }}
            onBlur={() => touched && onChange(draft)}
            className="size-5 cursor-default rounded-full border-0 bg-transparent p-0"
          />
          Custom
          {touched && draft !== value && (
            <button type="button" className="rounded px-1 text-accent-text hover:underline" onClick={() => onChange(draft)}>
              Apply
            </button>
          )}
        </label>
      </div>
    </div>
  );
}
