// SPDX-License-Identifier: GPL-3.0-or-later
import enApp from "../../../locales/en/app.json";
import enTray from "../../../locales/en/tray.json";
import nlApp from "../../../locales/nl/app.json";
import nlTray from "../../../locales/nl/tray.json";

export type CatalogLocale = "en" | "nl";

export const SOURCE_LOCALE: CatalogLocale = "en";

/** All messages per language (UI and tray), from `locales/<lang>/*.json`. */
export const catalogs: Record<CatalogLocale, Record<string, string>> = {
  en: { ...enApp, ...enTray },
  nl: { ...nlApp, ...nlTray },
};
