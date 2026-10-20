// SPDX-License-Identifier: GPL-3.0-or-later
// "kipple" with the dot of the i as a yolk egg (DESIGN.md §2): a dotless ı plus a drawn egg.
// The brand name isn't translated, so it isn't in the catalog.

export function Wordmark({ className = "" }: { className?: string }) {
  return (
    <span
      role="img"
      aria-label="Kipple"
      className={`inline-flex items-baseline font-display leading-none font-bold tracking-[-0.02em] font-soft ${className}`}
    >
      k
      <span className="relative after:absolute after:top-[-0.08em] after:left-1/2 after:h-[0.3em] after:w-[0.26em] after:-translate-x-[46%] after:rounded-egg after:bg-yolk after:content-['']">
        ı
      </span>
      pple
    </span>
  );
}
