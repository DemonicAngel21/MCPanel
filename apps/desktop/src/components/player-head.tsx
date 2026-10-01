import { useState } from "react";
import { cn } from "@/lib/utils";

export function PlayerHead({ name, uuid, className, size = "md" }: { name: string; uuid?: string | null; className?: string; size?: "sm" | "md" }) {
  const [unavailable, setUnavailable] = useState(false);
  const id = uuid?.replaceAll("-", "");
  const validUuid = !!id && /^[0-9a-f]{32}$/i.test(id);
  const dimension = size === "sm" ? "size-7" : "size-9";
  return (
    <span
      aria-hidden="true"
      className={cn(
        "player-head relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-lg border border-border-strong/70 bg-surface-3 font-mono font-semibold text-muted",
        dimension,
        size === "sm" ? "text-[10px]" : "text-xs",
        className,
      )}
    >
      {name.slice(0, 1).toUpperCase()}
      {validUuid && !unavailable && (
        <img
          src={`https://mc-heads.net/avatar/${id}/64`}
          alt=""
          loading="lazy"
          referrerPolicy="no-referrer"
          className="absolute inset-0 size-full image-pixelated"
          onError={() => setUnavailable(true)}
        />
      )}
    </span>
  );
}
