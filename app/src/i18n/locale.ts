// SPDX-License-Identifier: GPL-3.0-or-later
import type { CatalogLocale } from "./catalog";

/** The UI language for an OS locale such as `nl-BE` or `nl_NL`; English unless it's Dutch. */
export function pickLocale(osLocale: string | null): CatalogLocale {
  return osLocale?.toLowerCase().startsWith("nl") ? "nl" : "en";
}
