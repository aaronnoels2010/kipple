// SPDX-License-Identifier: GPL-3.0-or-later
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { render, screen, waitFor } from "@testing-library/react";
import { useIntl } from "react-intl";
import { afterEach, describe, expect, it, vi } from "vitest";

import { formatFiles } from "./format";
import { I18nProvider } from "./I18nProvider";

function Files() {
  return <p>{formatFiles(useIntl(), 12)}</p>;
}

function renderWithOsLocale(osLocale: string | null) {
  mockIPC((cmd) => (cmd === "get_locale" ? osLocale : undefined));
  render(
    <I18nProvider>
      <Files />
    </I18nProvider>,
  );
}

describe("I18nProvider", () => {
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
    document.documentElement.removeAttribute("lang");
  });

  it("uses the OS language reported by the backend", async () => {
    renderWithOsLocale("nl-BE");
    expect(await screen.findByText("12 bestanden")).toBeInTheDocument();
    await waitFor(() => expect(document.documentElement.lang).toBe("nl"));
  });

  it("falls back to English for other languages, even if the webview guessed Dutch", async () => {
    vi.spyOn(navigator, "language", "get").mockReturnValue("nl-NL");
    renderWithOsLocale("fr-BE");
    expect(await screen.findByText("12 files")).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("en");
  });
});
