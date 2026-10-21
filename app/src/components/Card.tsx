// SPDX-License-Identifier: GPL-3.0-or-later
import { type ReactNode, useId } from "react";

/** A titled section of a screen (DESIGN.md §8, "card"). */
export function Card({ title, children }: { title: ReactNode; children: ReactNode }) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="rounded-lg border border-border bg-surface p-4 shadow-1">
      <h2 id={id} className="mb-3 font-semibold">
        {title}
      </h2>
      {children}
    </section>
  );
}
