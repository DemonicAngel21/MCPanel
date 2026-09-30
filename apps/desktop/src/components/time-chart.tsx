import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement)
    .getPropertyValue(name.replace(/^var\((.*)\)$/, "$1"))
    .trim();
  return v || fallback;
}

/**
 * A time-series chart with axes, grid and a hover readout (uPlot, canvas-based).
 * `points` are [unixMs, value]; `windowMs` limits the x axis to the most recent span.
 */
export function TimeChart({
  points,
  windowMs,
  min = 0,
  max,
  color = "var(--accent)",
  format,
  threshold,
  height = 160,
  label,
}: {
  points: [number, number][];
  windowMs: number;
  min?: number;
  /** Fixed top of the y axis; otherwise 15 % above the largest value. */
  max?: number;
  color?: string;
  format: (v: number) => string;
  /** A dashed reference line (e.g. 50 ms per tick). */
  threshold?: number;
  height?: number;
  /** Accessible name of the chart. */
  label: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const latest = useRef({ points, windowMs, format, threshold });
  useEffect(() => {
    latest.current = { points, windowMs, format, threshold };
  });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const stroke = cssVar(color, "#22c55e");
    const grid = cssVar("--border", "#8884");
    const text = cssVar("--faint", "#888");
    const axis: uPlot.Axis = {
      stroke: text,
      grid: { stroke: grid, width: 1 },
      ticks: { stroke: grid, width: 1, size: 4 },
      font: "11px Inter, system-ui, sans-serif",
    };
    const opts: uPlot.Options = {
      width: el.clientWidth || 300,
      height,
      padding: [8, 8, 0, 0],
      legend: { show: false },
      cursor: { x: true, y: false, points: { size: 6, fill: stroke } },
      select: { show: false, left: 0, top: 0, width: 0, height: 0 },
      scales: {
        x: {
          time: true,
          range: (_u, _min, dataMax) => {
            const end = Math.max(dataMax ?? 0, Date.now() / 1000);
            return [end - latest.current.windowMs / 1000, end];
          },
        },
        y: {
          auto: false,
          range: (_u, _lo, dataMax) => [min, max ?? Math.max((dataMax ?? 0) * 1.15, latest.current.threshold ?? 0, 1)],
        },
      },
      axes: [
        { ...axis, space: 70 },
        { ...axis, size: 52, values: (_u, splits) => splits.map((v) => latest.current.format(v)) },
      ],
      series: [
        {},
        {
          label,
          stroke,
          width: 1.5,
          fill: `${stroke}1f`,
          points: { show: false },
          value: (_u, v) => (v == null ? "—" : latest.current.format(v)),
        },
      ],
      hooks: {
        draw: [
          (u) => {
            const t = latest.current.threshold;
            if (t == null) return;
            const y = u.valToPos(t, "y", true);
            const ctx = u.ctx;
            ctx.save();
            ctx.strokeStyle = cssVar("--warning", "#f59e0b");
            ctx.setLineDash([4, 4]);
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.moveTo(u.bbox.left, y);
            ctx.lineTo(u.bbox.left + u.bbox.width, y);
            ctx.stroke();
            ctx.restore();
          },
        ],
        setCursor: [
          (u) => {
            const i = u.cursor.idx;
            const tip = el.querySelector<HTMLElement>("[data-tip]");
            if (!tip) return;
            const v = i == null ? null : u.data[1]?.[i];
            if (i == null || v == null) {
              tip.style.opacity = "0";
              return;
            }
            const at = new Date((u.data[0]?.[i] ?? 0) * 1000).toLocaleTimeString();
            tip.textContent = `${latest.current.format(v)} · ${at}`;
            tip.style.opacity = "1";
          },
        ],
      },
    };
    const init = latest.current.points;
    plot.current = new uPlot(opts, [init.map((p) => p[0] / 1000), init.map((p) => p[1])], el);
    const ro = new ResizeObserver(() => plot.current?.setSize({ width: el.clientWidth, height }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      plot.current?.destroy();
      plot.current = null;
    };
  }, [height, min, max, color, label]);

  useEffect(() => {
    plot.current?.setData([points.map((p) => p[0] / 1000), points.map((p) => p[1])]);
  }, [points, windowMs, threshold]);

  return (
    <div ref={ref} role="img" aria-label={label} className="relative w-full" style={{ minHeight: height }}>
      <span
        data-tip
        aria-hidden
        className="pointer-events-none absolute top-0 right-2 z-10 rounded bg-surface-3 px-1.5 py-0.5 font-mono text-[11px] text-fg opacity-0 transition-opacity"
      />
    </div>
  );
}
