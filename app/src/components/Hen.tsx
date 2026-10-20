// SPDX-License-Identifier: GPL-3.0-or-later
// Kip, the mascot (docs/DESIGN.md §2). Geometric placeholder until a designer draws the final one.
// Never used in delete, error or permission dialogs: the safety voice stays calm.

export type HenState = "idle" | "happy" | "sleeping" | "alert" | "nesting";

const LINE = "var(--hen-line)";

function Face({ state }: { state: HenState }) {
  switch (state) {
    case "happy":
      return (
        <>
          <path d="M38.6 27c1.2-2.4 4.4-2.4 5.6 0" fill="none" stroke={LINE} strokeWidth="2.2" strokeLinecap="round" />
          <path d="M8 18l2 2m-4 4h3M12 12l1 3" stroke="var(--yolk)" strokeWidth="2" strokeLinecap="round" />
        </>
      );
    case "sleeping":
      return (
        <>
          <path d="M38.6 26.4c1.2 1.6 4.4 1.6 5.6 0" fill="none" stroke={LINE} strokeWidth="2.2" strokeLinecap="round" />
          <path d="M50 6h5l-5 6h5M57 1h3l-3 4h3" fill="none" stroke="var(--text-muted)" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
        </>
      );
    case "alert":
      return (
        <>
          <circle cx="41.5" cy="26" r="3.4" fill="var(--hen-eye)" stroke={LINE} strokeWidth="1.6" />
          <circle cx="42" cy="26" r="1.6" fill={LINE} />
          <path d="M8 10v9" stroke="var(--amber)" strokeWidth="3" strokeLinecap="round" />
          <circle cx="8" cy="24" r="1.8" fill="var(--amber)" />
        </>
      );
    case "nesting":
      return (
        <>
          <circle cx="41.5" cy="26" r="2.4" fill={LINE} />
          <ellipse cx="14" cy="48" rx="5" ry="6.4" fill="var(--hen)" stroke={LINE} strokeWidth="1.8" />
          <ellipse cx="9" cy="54" rx="4" ry="5" fill="var(--yolk-soft)" stroke={LINE} strokeWidth="1.6" />
        </>
      );
    default:
      return <circle cx="41.5" cy="26" r="2.4" fill={LINE} />;
  }
}

export function Hen({ state = "idle", className }: { state?: HenState; className?: string }) {
  return (
    <svg viewBox="0 0 64 64" aria-hidden="true" data-state={state} className={className}>
      <path d="M23 15.5c-.6-3.6 2.2-6 4.8-4.8.9-3.2 5.3-4.1 7.2-1.2 2.6-2 6.6-.4 6.2 3.2 2.4.3 3.1 3.3 1 4.8z" fill="var(--comb)" />
      <path
        d="M32 13.5c11.2 0 18.6 8.8 18.6 20.2 0 10.3-8.3 18.3-18.6 18.3s-18.6-8-18.6-18.3c0-11.4 7.4-20.2 18.6-20.2z"
        fill="var(--hen)"
        stroke={LINE}
        strokeWidth="2.2"
      />
      <path d="M49.4 27.4l7.4 3.3-7.2 3.2z" fill="var(--beak)" stroke={LINE} strokeWidth="1.6" strokeLinejoin="round" />
      <path d="M48.6 34.6c1.8.4 2.4 2.8.9 4.1-1.6 1.2-3.6-.3-3.2-2.2" fill="var(--comb)" />
      <path
        d="M17 32.5c.6 7.6 7.4 11.6 14.6 8.6 1.6-.7 1.2-2.3-.4-2.4-3.6-.3-6.2-2.2-7.6-5.2-1.3-2.7-6.8-3.6-6.6-1z"
        fill="var(--wing)"
        stroke={LINE}
        strokeWidth="2"
        strokeLinejoin="round"
      />
      <path d="M21.5 36.5c1.6 1.6 3.4 2.4 5.4 2.6" fill="none" stroke={LINE} strokeWidth="1.4" strokeLinecap="round" opacity="0.55" />
      <path d="M27.5 52v6.2m-3.4 0h6.6M36.5 52v6.2m-3.4 0h6.6" stroke="var(--beak)" strokeWidth="2.4" strokeLinecap="round" />
      <Face state={state} />
    </svg>
  );
}
