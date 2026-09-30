import * as SliderPrimitive from "@radix-ui/react-slider";
import { Input } from "@/components/ui/primitives";
import { cn } from "@/lib/utils";

const STEP = 256;
/** Memory Windows and other programs need; allocating into it slows the whole PC. */
const RESERVE_MB = 2048;

function formatMb(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1)} GB` : `${mb} MB`;
}

/**
 * Minimum and maximum Java heap as a two-thumb slider (256 MB steps) with exact inputs.
 * The part of the track that would eat into the memory Windows needs is marked.
 */
export function MemoryRange({
  minMb,
  maxMb,
  totalMb,
  onChange,
}: {
  minMb: number;
  maxMb: number;
  /** This computer's memory; `null` while unknown. */
  totalMb: number | null;
  onChange: (minMb: number, maxMb: number) => void;
}) {
  const cap = Math.max(totalMb ? Math.floor(totalMb / STEP) * STEP : 16384, maxMb, 1024);
  const safe = totalMb ? Math.max(1024, totalMb - RESERVE_MB) : cap;
  const safePct = Math.min(100, (safe / cap) * 100);
  const risky = totalMb != null && maxMb > safe;
  const ticks = [0, 0.25, 0.5, 0.75, 1].map((f) => Math.round((cap * f) / STEP) * STEP);

  const setMin = (v: number) => onChange(Math.min(v, maxMb), maxMb);
  const setMax = (v: number) => onChange(Math.min(minMb, v), v);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <p className="text-[13px] text-fg">
          <span className="font-semibold tabular-nums">{formatMb(minMb)}</span>
          <span className="text-muted"> to </span>
          <span className={cn("font-semibold tabular-nums", risky && "text-warning")}>{formatMb(maxMb)}</span>
        </p>
        {totalMb != null && <p className="text-xs text-muted">This computer has {formatMb(totalMb)}</p>}
      </div>
      <SliderPrimitive.Root
        className="relative flex h-5 w-full touch-none items-center select-none"
        min={0}
        max={cap}
        step={STEP}
        minStepsBetweenThumbs={0}
        value={[minMb, maxMb]}
        onValueChange={([lo, hi]) => onChange(lo ?? minMb, Math.max(hi ?? maxMb, 512))}
      >
        <SliderPrimitive.Track className="relative h-2 w-full grow overflow-hidden rounded-full bg-surface-3">
          {totalMb != null && safePct < 100 && (
            <div aria-hidden className="absolute inset-y-0 right-0 bg-warning/25" style={{ left: `${safePct}%` }} />
          )}
          <SliderPrimitive.Range className="absolute h-full bg-accent" />
        </SliderPrimitive.Track>
        {(["Minimum memory", "Maximum memory"] as const).map((label) => (
          <SliderPrimitive.Thumb
            key={label}
            aria-label={label}
            className="block size-4 rounded-full border-2 border-accent bg-surface shadow transition-[scale] duration-100 hover:scale-110 focus-visible:scale-110"
          />
        ))}
      </SliderPrimitive.Root>
      <div aria-hidden className="flex justify-between text-[10px] text-faint tabular-nums">
        {ticks.map((t) => (
          <span key={t}>{formatMb(t)}</span>
        ))}
      </div>
      <div className="grid grid-cols-2 gap-3">
        <label className="space-y-1">
          <span className="text-xs text-muted">Minimum (MB)</span>
          <Input inputMode="numeric" value={minMb} onChange={(e) => setMin(Number(e.target.value.replace(/\D/g, "")) || 0)} />
        </label>
        <label className="space-y-1">
          <span className="text-xs text-muted">Maximum (MB)</span>
          <Input inputMode="numeric" value={maxMb} onChange={(e) => setMax(Number(e.target.value.replace(/\D/g, "")) || 0)} />
        </label>
      </div>
      {risky && (
        <p className="text-xs text-warning">
          More than {formatMb(safe)} leaves Windows less than {formatMb(RESERVE_MB)}; the computer and the server may slow down.
        </p>
      )}
    </div>
  );
}
