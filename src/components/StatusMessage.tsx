import type { ReactNode } from "react";
import { cn } from "../lib/utils";

export function StatusMessage({
  tone = "info",
  children,
  action,
  live,
  className,
}: {
  tone?: "info" | "success" | "error";
  children: ReactNode;
  action?: ReactNode;
  live?: boolean;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "rounded-lg border px-4 py-3 text-sm",
        tone === "error" && "border-red-500/30 bg-red-500/8 text-red-200",
        tone === "success" && "border-accent/30 bg-accent/8 text-text",
        tone === "info" && "border-border bg-surface text-muted",
        className,
      )}
      role={live ? (tone === "error" ? "alert" : "status") : undefined}
      aria-live={live ? "polite" : undefined}
      aria-atomic={live ? "true" : undefined}
    >
      <div className="flex flex-wrap items-center gap-3">
        <div className="min-w-0 flex-1">{children}</div>
        {action}
      </div>
    </div>
  );
}