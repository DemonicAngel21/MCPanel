import { cn } from "@/lib/utils";

type SoftwareMarkProps = { softwareId?: string | null; name?: string; className?: string; size?: "sm" | "md" | "lg" };

const marks: Record<
  string,
  { label: string; tone: string; shape: "block" | "paper" | "layers" | "weave" | "anvil" | "nodes" | "quilt" | "spigot" | "bucket" }
> = {
  vanilla: { label: "Vanilla", tone: "software-vanilla", shape: "block" },
  paper: { label: "Paper", tone: "software-paper", shape: "paper" },
  purpur: { label: "Purpur", tone: "software-purpur", shape: "layers" },
  fabric: { label: "Fabric", tone: "software-fabric", shape: "weave" },
  forge: { label: "Forge", tone: "software-forge", shape: "anvil" },
  neoforge: { label: "NeoForge", tone: "software-neoforge", shape: "nodes" },
  quilt: { label: "Quilt", tone: "software-quilt", shape: "quilt" },
  spigot: { label: "Spigot", tone: "software-spigot", shape: "spigot" },
  bukkit: { label: "Bukkit", tone: "software-bukkit", shape: "bucket" },
};

function MarkShape({ shape }: { shape: (typeof marks)[string]["shape"] }) {
  const common = { fill: "none", stroke: "currentColor", strokeWidth: 1.7, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  switch (shape) {
    case "block":
      return (
        <>
          <path d="m12 2.8 8 4.4v9.5l-8 4.5-8-4.5V7.2z" {...common} />
          <path d="m4.3 7.2 7.7 4.4 7.7-4.4M12 11.6v9.2" {...common} />
        </>
      );
    case "paper":
      return (
        <>
          <path d="M6 3.5h8l4 4v13H6z" {...common} />
          <path d="M14 3.8v4h4M9 12h6M9 15.5h6" {...common} />
        </>
      );
    case "layers":
      return (
        <>
          <path d="m12 3 8 4.5-8 4.5-8-4.5zM4 12l8 4.5 8-4.5M4 16.5l8 4.5 8-4.5" {...common} />
        </>
      );
    case "weave":
      return (
        <>
          <path d="M5 5h14v14H5zM5 9h14M5 15h14M9 5v14M15 5v14" {...common} />
        </>
      );
    case "anvil":
      return (
        <>
          <path d="M4 8h16l-2 4h-5l-1.5 4H8l1-4H6zM8 19h9" {...common} />
          <path d="M8 5h8" {...common} />
        </>
      );
    case "nodes":
      return (
        <>
          <circle cx="12" cy="5" r="2.2" {...common} />
          <circle cx="6" cy="17" r="2.2" {...common} />
          <circle cx="18" cy="17" r="2.2" {...common} />
          <path d="m11 7-4 8m6-8 4 8M8.5 17h7" {...common} />
        </>
      );
    case "quilt":
      return (
        <>
          <path d="M5 5h6v6H5zM13 5h6v6h-6zM5 13h6v6H5zM13 13h6v6h-6z" {...common} />
          <path d="M8 5v6m8-6v6m-8 2v6m8-6v6" {...common} />
        </>
      );
    case "spigot":
      return (
        <>
          <path d="M5 8h14v4H5zM9 12v5h6v-5M12 4v4M9 4h6" {...common} />
          <path d="M12 17c-1.4 1.3-1.4 2.7 0 3.2 1.4-.5 1.4-1.9 0-3.2Z" {...common} />
        </>
      );
    case "bucket":
      return (
        <>
          <path d="M6 8h12l-1 12H7zM8 8a4 4 0 0 1 8 0" {...common} />
          <path d="M9 12h6" {...common} />
        </>
      );
  }
}

export function SoftwareMark({ softwareId, name, className, size = "md" }: SoftwareMarkProps) {
  const mark = marks[(softwareId ?? "").toLowerCase()] ?? {
    label: name ?? softwareId ?? "Server",
    tone: "software-default",
    shape: "block" as const,
  };
  return (
    <span
      aria-hidden="true"
      title={mark.label}
      className={cn(
        "software-mark inline-flex shrink-0 items-center justify-center rounded-lg",
        mark.tone,
        size === "sm" && "size-7 rounded-md [&_svg]:size-4",
        size === "md" && "size-9 [&_svg]:size-5",
        size === "lg" && "size-11 [&_svg]:size-6",
        className,
      )}
    >
      <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <MarkShape shape={mark.shape} />
      </svg>
    </span>
  );
}
