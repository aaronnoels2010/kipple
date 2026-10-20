// SPDX-License-Identifier: GPL-3.0-or-later

export function Kbd({ children, className = "" }: { children: string; className?: string }) {
  return (
    <kbd
      className={`rounded-[5px] border border-b-2 border-border bg-surface-2 px-1.5 py-0.5 font-mono text-[10.5px] leading-[1.3] font-medium text-muted [font-stretch:87.5%] ${className}`}
    >
      {children}
    </kbd>
  );
}
