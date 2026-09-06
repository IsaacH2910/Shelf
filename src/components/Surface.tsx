import type { HTMLAttributes } from "react";
import { cn } from "../lib/utils";

export function Surface({
  className,
  padded = true,
  ...props
}: HTMLAttributes<HTMLDivElement> & { padded?: boolean }) {
  return (
    <div
      className={cn(
        "rounded-xl border border-border bg-surface",
        padded && "px-4 py-3 md:px-5 md:py-4",
        className,
      )}
      {...props}
    />
  );
}