import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

/** Minimal uPlot time-series (fast, canvas-based). `points` are [unixMs, value]. */
export function Sparkline({
  points,
  max,
  height = 56,
  color = "var(--accent)",
  format,
}: {
  points: [number, number][];
  max?: number;
  height?: number;
  color?: string;
  format?: (v: number) => string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  // Callers pass inline `format` functions and new `points` arrays on every render; keep
  // the latest in refs so the chart is only rebuilt when its shape changes.
  const formatRef = useRef(format);
  const pointsRef = useRef(points);
  useEffect(() => {
    formatRef.current = format;
    pointsRef.current = points;
  });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const resolved =
      getComputedStyle(document.documentElement)
        .getPropertyValue(color.replace(/^var\((.*)\)$/, "$1"))
        .trim() || color;
    const opts: uPlot.Options = {
      width: el.clientWidth || 200,
      height,
      padding: [4, 0, 0, 0],
      cursor: { show: true, x: false, y: false, points: { show: false } },
      legend: { show: false },
      select: { show: false, left: 0, top: 0, width: 0, height: 0 },
      scales: {
        x: { time: true },
        y: { auto: false, range: max ? [0, max] : (_u, _min, dataMax) => [0, Math.max((dataMax ?? 0) * 1.15, 1)] },
      },
      axes: [{ show: false }, { show: false }],
      series: [
        {},
        {
          stroke: resolved,
          width: 1.5,
          points: { show: false },
          fill: `${resolved}22`,
          value: (_u, v) => (v == null ? "—" : formatRef.current ? formatRef.current(v) : String(v)),
        },
      ],
    };
    const initial = pointsRef.current;
    plot.current = new uPlot(opts, [initial.map((p) => p[0] / 1000), initial.map((p) => p[1])], el);
    const ro = new ResizeObserver(() => plot.current?.setSize({ width: el.clientWidth, height }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      plot.current?.destroy();
      plot.current = null;
    };
  }, [height, max, color]);

  useEffect(() => {
    plot.current?.setData([points.map((p) => p[0] / 1000), points.map((p) => p[1])]);
  }, [points]);

  return (
    <div className="relative w-full" style={{ height }}>
      <div ref={ref} className="absolute inset-0" />
      {/* Until there are two samples, show a quiet baseline instead of blank space. */}
      {points.length < 2 && <div aria-hidden className="absolute inset-x-0 bottom-0 border-b border-dashed border-border-strong" />}
    </div>
  );
}
