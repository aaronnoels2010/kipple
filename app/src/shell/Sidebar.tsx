// SPDX-License-Identifier: GPL-3.0-or-later
import { FormattedMessage, useIntl } from "react-intl";

import { Wordmark } from "../components/Wordmark";
import { type Section, section, SECTIONS, type SectionId } from "./sections";

function NavItem({ item, active, onSelect }: { item: Section; active: boolean; onSelect: (id: SectionId) => void }) {
  const Icon = item.icon;
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={() => onSelect(item.id)}
      className={`flex min-h-[34px] w-full items-center gap-2.5 rounded-md border px-2.5 text-start font-[550] transition-colors duration-[120ms] ${
        active
          ? "border-border bg-surface text-text shadow-1"
          : "border-transparent text-muted hover:bg-surface-2 hover:text-text"
      }`}
    >
      <Icon size={16} strokeWidth={1.9} className={`shrink-0 ${active ? "text-yolk-ink" : ""}`} />
      <span>
        <FormattedMessage id={item.label} />
      </span>
    </button>
  );
}

/** The 228 px sidebar (DESIGN.md §7): wordmark, sections, and Settings at the bottom. */
export function Sidebar({ current, onSelect }: { current: SectionId; onSelect: (id: SectionId) => void }) {
  const intl = useIntl();
  const settings = section("settings");
  return (
    <aside className="flex min-h-0 flex-col border-e border-border bg-sidebar px-2.5 py-3.5">
      <Wordmark className="mx-2.5 mt-1 mb-3.5 text-2xl" />
      <nav aria-label={intl.formatMessage({ id: "nav.label" })} className="flex min-h-0 flex-1 flex-col gap-1">
        {SECTIONS.filter((item) => item !== settings).map((item) => (
          <NavItem key={item.id} item={item} active={item.id === current} onSelect={onSelect} />
        ))}
        <div className="mt-auto">
          <NavItem item={settings} active={settings.id === current} onSelect={onSelect} />
        </div>
      </nav>
    </aside>
  );
}
