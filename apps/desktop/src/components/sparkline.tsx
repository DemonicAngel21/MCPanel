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
          value: (_u, v) => (v == null ? "—" : format ? format(v) : String(v)),
        },
      ],
    };
    plot.current = new uPlot(opts, [[], []], el);
    const ro = new ResizeObserver(() => plot.current?.setSize({ width: el.clientWidth, height }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      plot.current?.destroy();
      plot.current = null;
    };
  }, [height, max, color, format]);

  useEffect(() => {
    plot.current?.setData([points.map((p) => p[0] / 1000), points.map((p) => p[1])]);
  }, [points]);

  return <div ref={ref} className="w-full" style={{ height }} />;
}
