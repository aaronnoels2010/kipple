// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect } from "react";

import { hasModifier } from "./platform";
import { SECTIONS, type SectionId } from "./sections";

export interface ShortcutHandlers {
  onSection: (id: SectionId) => void;
  onSearch: () => void;
  onHelp: () => void;
  /** While a dialog is open, only `?` (and the dialog's own Esc) work. */
  paused: boolean;
}

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  return target instanceof HTMLInputElement && !["checkbox", "radio", "button"].includes(target.type);
}

/** The app-wide keys from DESIGN.md §11. Screen-specific keys (Space, K, P…) live with their screens. */
export function useShortcuts({ onSection, onSearch, onHelp, paused }: ShortcutHandlers) {
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.repeat || e.isComposing) return;
      if (e.key === "?" && !e.ctrlKey && !e.metaKey && !isTyping(e.target)) {
        e.preventDefault();
        onHelp();
        return;
      }
      if (paused || !hasModifier(e) || e.altKey || e.shiftKey) return;
      // The physical key: on AZERTY, Ctrl+1 reports key "&" but code "Digit1".
      const digit = /^Digit([1-9])$/.exec(e.code)?.[1];
      const target = digit && SECTIONS[Number(digit) - 1];
      if (target) {
        e.preventDefault();
        onSection(target.id);
      } else if (e.key.toLowerCase() === "k") {
        e.preventDefault();
        onSearch();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onSection, onSearch, onHelp, paused]);
}
