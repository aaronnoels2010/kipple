// SPDX-License-Identifier: GPL-3.0-or-later
// The app's sections in sidebar order; Ctrl/⌘ 1…7 follow this order (DESIGN.md §11).
import {
  FolderOpen,
  FolderTree,
  History,
  LayoutDashboard,
  type LucideIcon,
  PencilLine,
  Settings,
  Sparkles,
} from "lucide-react";

import type { HenState } from "../components/Hen";

export type SectionId = "overview" | "browse" | "cleanup" | "rename" | "tidy" | "history" | "settings";

export interface Section {
  id: SectionId;
  icon: LucideIcon;
  label: string;
  /** What the screen shows until there's data (all but Settings, for now). DESIGN.md §2: the
   * nesting hen for empty lists, the happy one for "all clean". */
  empty?: { hen: HenState; title: string; body: string };
}

export const SECTIONS: readonly Section[] = [
  {
    id: "overview",
    icon: LayoutDashboard,
    label: "nav.overview",
    empty: { hen: "nesting", title: "empty.overview.title", body: "empty.overview.body" },
  },
  {
    id: "browse",
    icon: FolderOpen,
    label: "nav.browse",
    empty: { hen: "nesting", title: "empty.browse.title", body: "empty.browse.body" },
  },
  {
    id: "cleanup",
    icon: Sparkles,
    label: "nav.cleanup",
    empty: { hen: "happy", title: "empty.cleanup.title", body: "empty.cleanup.body" },
  },
  {
    id: "rename",
    icon: PencilLine,
    label: "nav.rename",
    empty: { hen: "nesting", title: "empty.rename.title", body: "empty.rename.body" },
  },
  {
    id: "tidy",
    icon: FolderTree,
    label: "nav.tidy",
    empty: { hen: "nesting", title: "empty.tidy.title", body: "empty.tidy.body" },
  },
  {
    id: "history",
    icon: History,
    label: "nav.history",
    empty: { hen: "nesting", title: "empty.history.title", body: "empty.history.body" },
  },
  { id: "settings", icon: Settings, label: "nav.settings" },
];

export function section(id: SectionId): Section {
  return SECTIONS.find((s) => s.id === id)!;
}
