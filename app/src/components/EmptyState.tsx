// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

import { Hen, type HenState } from "./Hen";

/** The hen, a title and one calm sentence: what every list shows before there's data. */
export function EmptyState({ hen, title, body }: { hen: HenState; title: ReactNode; body: ReactNode }) {
  return (
    <div className="grid justify-items-center gap-1.5 px-5 py-7 text-center">
      <Hen state={hen} className="size-24" />
      <h2 className="mt-1.5 font-display text-xl font-semibold font-soft">{title}</h2>
      <p className="max-w-[38ch] text-muted">{body}</p>
    </div>
  );
}
