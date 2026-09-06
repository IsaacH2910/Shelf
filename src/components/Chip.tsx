import type { ButtonHTMLAttributes } from "react";
import { cn } from "../lib/utils";

export function Chip({
  className,
  active,
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { active?: boolean }) {
  return (
    <button
      type={type}
      aria-pressed={active}
      className={cn(
        "shrink-0 rounded-lg px-3 py-1.5 text-xs transition-colors focus:outline-none focus:ring-1 focus:ring-accent",
        active ? "bg-accent text-bg" : "bg-elevated text-muted hover:text-text",
        className,
      )}
      {...props}
    />
  );
}