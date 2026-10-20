// SPDX-License-Identifier: GPL-3.0-or-later
import type { LucideIcon } from "lucide-react";
import type { ComponentProps } from "react";

/** An icon-only button: `label` is both its accessible name and its tooltip (DESIGN.md §6). */
export function IconButton({
  label,
  icon: Icon,
  className = "",
  ...props
}: { label: string; icon: LucideIcon } & ComponentProps<"button">) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`inline-flex size-7 shrink-0 items-center justify-center rounded-sm hover:bg-surface-2 ${className}`}
      {...props}
    >
      <Icon size={16} strokeWidth={1.9} />
    </button>
  );
}
