import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import * as LabelPrimitive from "@radix-ui/react-label";
import * as ProgressPrimitive from "@radix-ui/react-progress";
import * as SwitchPrimitive from "@radix-ui/react-switch";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { Check, Loader2 } from "lucide-react";
import { forwardRef, type HTMLAttributes, type InputHTMLAttributes, type ReactNode, type TextareaHTMLAttributes } from "react";
import { cn } from "@/lib/utils";
import type { Tone } from "@/lib/server-state";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      "h-8 w-full rounded-md border border-border-strong bg-surface-2 px-2.5 text-[13px] text-fg placeholder:text-faint",
      "focus:border-accent/60 focus:ring-2 focus:ring-ring/30 focus:outline-none disabled:opacity-50",
      className,
    )}
    spellCheck={false}
    {...props}
  />
));
Input.displayName = "Input";

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(({ className, ...props }, ref) => (
  <textarea
    ref={ref}
    className={cn(
      "min-h-16 w-full rounded-md border border-border-strong bg-surface-2 px-2.5 py-1.5 text-[13px] text-fg placeholder:text-faint",
      "focus:border-accent/60 focus:ring-2 focus:ring-ring/30 focus:outline-none",
      className,
    )}
    spellCheck={false}
    {...props}
  />
));
Textarea.displayName = "Textarea";

export const Label = forwardRef<HTMLLabelElement, LabelPrimitive.LabelProps>(({ className, ...props }, ref) => (
  <LabelPrimitive.Root ref={ref} className={cn("text-xs font-medium text-muted", className)} {...props} />
));
Label.displayName = "Label";

export function Field({
  label,
  hint,
  error,
  children,
  className,
}: {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <Label>{label}</Label>
      {children}
      {error ? <p className="text-xs text-danger">{error}</p> : hint ? <p className="text-xs text-faint">{hint}</p> : null}
    </div>
  );
}

export const Switch = forwardRef<HTMLButtonElement, SwitchPrimitive.SwitchProps>(({ className, ...props }, ref) => (
  <SwitchPrimitive.Root
    ref={ref}
    className={cn(
      "inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border border-transparent transition-colors",
      "disabled:opacity-50 data-[state=checked]:bg-accent data-[state=unchecked]:bg-border-strong",
      className,
    )}
    {...props}
  >
    <SwitchPrimitive.Thumb className="block size-3.5 translate-x-0.5 rounded-full bg-white shadow transition-transform data-[state=checked]:translate-x-[15px]" />
  </SwitchPrimitive.Root>
));
Switch.displayName = "Switch";

export const Checkbox = forwardRef<HTMLButtonElement, CheckboxPrimitive.CheckboxProps>(({ className, ...props }, ref) => (
  <CheckboxPrimitive.Root
    ref={ref}
    className={cn(
      "flex size-4 shrink-0 items-center justify-center rounded border border-border-strong bg-surface-2",
      "data-[state=checked]:border-accent data-[state=checked]:bg-accent data-[state=checked]:text-accent-fg",
      className,
    )}
    {...props}
  >
    <CheckboxPrimitive.Indicator>
      <Check className="size-3" strokeWidth={3} />
    </CheckboxPrimitive.Indicator>
  </CheckboxPrimitive.Root>
));
Checkbox.displayName = "Checkbox";

const toneClasses: Record<Tone, string> = {
  neutral: "bg-surface-3 text-muted",
  success: "bg-accent-soft text-accent",
  warning: "bg-warning-soft text-warning",
  danger: "bg-danger-soft text-danger",
  info: "bg-info-soft text-info",
};

export function Badge({ tone = "neutral", className, children, ...props }: HTMLAttributes<HTMLSpanElement> & { tone?: Tone }) {
  return (
    <span className={cn("inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium", toneClasses[tone], className)} {...props}>
      {children}
    </span>
  );
}

const dotClasses: Record<Tone, string> = {
  neutral: "bg-faint",
  success: "bg-accent",
  warning: "bg-warning",
  danger: "bg-danger",
  info: "bg-info",
};

export function StatusDot({ tone, pulse }: { tone: Tone; pulse?: boolean }) {
  return (
    <span className="relative inline-flex size-2">
      {pulse && <span className={cn("absolute inset-0 animate-ping rounded-full opacity-60", dotClasses[tone])} />}
      <span className={cn("relative inline-flex size-2 rounded-full", dotClasses[tone])} />
    </span>
  );
}

export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("rounded-lg border border-border bg-surface", className)} {...props} />;
}

export function CardHeader({
  title,
  description,
  actions,
  className,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex items-start justify-between gap-3 border-b border-border px-4 py-3", className)}>
      <div className="min-w-0">
        <h3 className="text-[13px] font-semibold text-fg">{title}</h3>
        {description && <p className="mt-0.5 text-xs text-muted">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn("size-4 animate-spin text-muted", className)} />;
}

export function Progress({ value, className }: { value: number | null; className?: string }) {
  return (
    <ProgressPrimitive.Root className={cn("relative h-1.5 w-full overflow-hidden rounded-full bg-surface-3", className)} value={value ?? undefined}>
      <ProgressPrimitive.Indicator
        className={cn("h-full bg-accent transition-[width] duration-300", value == null && "w-1/3 animate-pulse")}
        style={value != null ? { width: `${Math.round(Math.min(1, Math.max(0, value)) * 100)}%` } : undefined}
      />
    </ProgressPrimitive.Root>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded border border-border-strong bg-surface-2 px-1.5 py-px font-mono text-[10px] text-muted">{children}</kbd>;
}

export const TooltipProvider = TooltipPrimitive.Provider;

export function Tooltip({
  content,
  children,
  side = "top",
}: {
  content: ReactNode;
  children: ReactNode;
  side?: "top" | "bottom" | "left" | "right";
}) {
  if (!content) return <>{children}</>;
  return (
    <TooltipPrimitive.Root delayDuration={300}>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={side}
          sideOffset={6}
          className="z-50 max-w-xs animate-fade-in rounded-md border border-border-strong bg-surface-3 px-2 py-1 text-xs text-fg shadow-lg"
        >
          {content}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}

export function EmptyState({ icon, title, description, action }: { icon?: ReactNode; title: string; description?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-12 text-center">
      {icon && <div className="mb-1 text-faint [&_svg]:size-8">{icon}</div>}
      <p className="text-sm font-medium text-fg">{title}</p>
      {description && <p className="max-w-sm text-xs text-muted">{description}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function Banner({
  tone,
  icon,
  title,
  children,
  actions,
}: {
  tone: Tone;
  icon?: ReactNode;
  title: ReactNode;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  const border: Record<Tone, string> = {
    neutral: "border-border",
    success: "border-accent/30",
    warning: "border-warning/30",
    danger: "border-danger/30",
    info: "border-info/30",
  };
  return (
    <div className={cn("flex items-start gap-3 rounded-lg border px-3.5 py-3", toneClasses[tone], border[tone])}>
      {icon && <div className="mt-0.5 shrink-0 [&_svg]:size-4">{icon}</div>}
      <div className="min-w-0 flex-1">
        <p className="text-[13px] font-medium">{title}</p>
        {children && <div className="mt-0.5 text-xs text-fg/80">{children}</div>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}
