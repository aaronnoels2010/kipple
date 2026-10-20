// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useRef, useState } from "react";
import { FormattedMessage } from "react-intl";

import { EmptyState } from "./components/EmptyState";
import { SettingsScreen } from "./screens/SettingsScreen";
import { CheatSheet } from "./shell/CheatSheet";
import { section, type SectionId } from "./shell/sections";
import { Sidebar } from "./shell/Sidebar";
import { TopBar } from "./shell/TopBar";
import { useShortcuts } from "./shell/useShortcuts";
import { useAppearance } from "./theme/useAppearance";

interface Navigation {
  section: SectionId;
  /** Keyboard navigation moves focus to the new page title, so screen readers announce it. */
  focusTitle: boolean;
}

export default function App() {
  useAppearance();
  const [nav, setNav] = useState<Navigation>({ section: "overview", focusTitle: false });
  const [helpOpen, setHelpOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    if (nav.focusTitle) titleRef.current?.focus();
  }, [nav]);

  const goTo = useCallback((id: SectionId) => setNav({ section: id, focusTitle: true }), []);
  useShortcuts({
    onSection: goTo,
    onSearch: useCallback(() => searchRef.current?.focus(), []),
    onHelp: useCallback(() => setHelpOpen((open) => !open), []),
    paused: helpOpen,
  });

  const { label, empty } = section(nav.section);
  return (
    <div className="grid h-screen grid-cols-[228px_1fr] bg-bg text-text">
      <Sidebar current={nav.section} onSelect={(id) => setNav({ section: id, focusTitle: false })} />
      <div className="flex min-h-0 min-w-0 flex-col">
        <TopBar searchRef={searchRef} onHistory={() => goTo("history")} />
        <main className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto px-6 py-[22px]">
          {/* Not interactive, so no focus ring: it's only focused to be announced. */}
          <h1
            ref={titleRef}
            tabIndex={-1}
            className="font-display text-[28px] leading-tight font-semibold tracking-[-0.02em] font-soft outline-none"
          >
            <FormattedMessage id={label} />
          </h1>
          {empty ? (
            <div className="grid flex-1 place-content-center">
              <EmptyState
                hen={empty.hen}
                title={<FormattedMessage id={empty.title} />}
                body={<FormattedMessage id={empty.body} />}
              />
            </div>
          ) : (
            <SettingsScreen />
          )}
        </main>
      </div>
      <CheatSheet open={helpOpen} onOpenChange={setHelpOpen} />
    </div>
  );
}
