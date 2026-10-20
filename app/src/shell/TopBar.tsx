// SPDX-License-Identifier: GPL-3.0-or-later
import { History, Search } from "lucide-react";
import type { Ref } from "react";
import { useIntl } from "react-intl";

import { IconButton } from "../components/IconButton";
import { Kbd } from "../components/Kbd";
import { shortcutLabel } from "./platform";

/** The 56 px top bar (DESIGN.md §7): search and History. "Scan now" arrives with the scanner. */
export function TopBar({ searchRef, onHistory }: { searchRef: Ref<HTMLInputElement>; onHistory: () => void }) {
  const intl = useIntl();
  const placeholder = intl.formatMessage({ id: "search.placeholder" });
  return (
    <header className="flex h-14 shrink-0 items-center gap-2.5 border-b border-border px-[18px]">
      <div className="relative flex w-[380px] max-w-full items-center">
        <Search size={16} strokeWidth={1.9} className="pointer-events-none absolute start-[11px] text-muted" />
        <input
          ref={searchRef}
          type="search"
          aria-label={placeholder}
          placeholder={placeholder}
          className="h-[34px] w-full rounded-full border border-transparent bg-surface-2 ps-[34px] pe-[64px] text-text placeholder:text-muted hover:border-border-strong focus:border-focus"
        />
        <Kbd className="pointer-events-none absolute end-2">{shortcutLabel("K")}</Kbd>
      </div>
      <IconButton
        label={intl.formatMessage({ id: "topbar.history" })}
        icon={History}
        onClick={onHistory}
        className="ms-auto text-text"
      />
    </header>
  );
}
