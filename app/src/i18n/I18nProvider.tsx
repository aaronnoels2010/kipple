// SPDX-License-Identifier: GPL-3.0-or-later
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { IntlProvider } from "react-intl";

import { commands } from "../bindings";
import { type CatalogLocale, catalogs, SOURCE_LOCALE } from "./catalog";
import { pickLocale } from "./locale";
import { pseudoLocalize } from "./pseudo";

// `VITE_KIPPLE_LOCALE=pseudo` turns on the pseudo-locale for checking untranslated/clipped text.
// Replaced at build time, so release builds drop pseudo.ts.
const PSEUDO = import.meta.env.VITE_KIPPLE_LOCALE === "pseudo";

function pseudoMessages(): Record<string, string> {
  return Object.fromEntries(Object.entries(catalogs[SOURCE_LOCALE]).map(([key, m]) => [key, pseudoLocalize(m)]));
}

/** Provides translations in the OS language (from Rust), falling back to English. */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [osLocale, setOsLocale] = useState<CatalogLocale>(() => pickLocale(navigator.language));

  useEffect(() => {
    let mounted = true;
    commands
      .getLocale()
      .then((os) => mounted && setOsLocale(pickLocale(os)))
      .catch(() => {}); // outside Tauri: keep the navigator.language guess
    return () => {
      mounted = false;
    };
  }, []);

  const locale = PSEUDO ? SOURCE_LOCALE : osLocale;
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  const messages = useMemo(() => (PSEUDO ? pseudoMessages() : catalogs[locale]), [locale]);
  return (
    <IntlProvider locale={locale} defaultLocale={SOURCE_LOCALE} messages={messages}>
      {children}
    </IntlProvider>
  );
}
