// SPDX-License-Identifier: GPL-3.0-or-later
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import { I18nProvider } from "./i18n/I18nProvider";
import { resetAppearanceStore } from "./theme/useAppearance";

const BACKEND: Record<string, unknown> = {
  app_info: { version: "1.2.3", os: "macos" },
  get_appearance: { preference: "system", effective: "light" },
  get_locale: "en-US",
};

type Handler = (cmd: string, args: unknown) => unknown;

// Mocks Tauri's IPC layer, so the generated bindings (command names, arguments) are exercised too.
function mockBackend(handler: Handler = (cmd) => BACKEND[cmd]) {
  const calls: string[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push(cmd);
      return handler(cmd, args);
    },
    { shouldMockEvents: true },
  );
  return calls;
}

function renderApp() {
  return render(
    <I18nProvider>
      <App />
    </I18nProvider>,
  );
}

const sidebar = () => screen.getByRole("navigation");
const pageTitle = () => screen.getByRole("heading", { level: 1 });
// Digits carry their physical `code` too: on AZERTY (Belgium, France) Ctrl+1 reports key "&".
const press = (key: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(window, { key, code: /^\d$/.test(key) ? `Digit${key}` : undefined, ...init });

describe("App shell", () => {
  afterEach(() => {
    resetAppearanceStore();
    clearMocks();
    vi.restoreAllMocks();
  });

  it("shows the wordmark and every section in the sidebar, starting on Overview", () => {
    mockBackend();
    renderApp();
    expect(screen.getByRole("img", { name: "Kipple" })).toBeInTheDocument();
    expect(sidebar()).toHaveAccessibleName("Sections");
    const items = within(sidebar()).getAllByRole("button");
    expect(items.map((b) => b.textContent)).toEqual([
      "Overview",
      "Browse",
      "Cleanup",
      "Rename",
      "Tidy",
      "History",
      "Settings",
    ]);
    expect(within(sidebar()).getByRole("button", { name: "Overview" })).toHaveAttribute("aria-current", "page");
    expect(pageTitle()).toHaveTextContent("Overview");
  });

  it("switches screens from the sidebar, with the hen in the empty state", () => {
    mockBackend();
    renderApp();
    fireEvent.click(within(sidebar()).getByRole("button", { name: "Cleanup" }));
    expect(pageTitle()).toHaveTextContent("Cleanup");
    expect(within(sidebar()).getByRole("button", { name: "Cleanup" })).toHaveAttribute("aria-current", "page");
    expect(within(sidebar()).getByRole("button", { name: "Overview" })).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("heading", { level: 2, name: "Not a speck of kipple." })).toBeInTheDocument();
    expect(screen.getByRole("main").querySelector('svg[data-state="happy"]')).not.toBeNull();
  });

  it("gives every screen except Settings a hen empty state", () => {
    mockBackend();
    renderApp();
    for (const name of ["Overview", "Browse", "Cleanup", "Rename", "Tidy", "History"]) {
      fireEvent.click(within(sidebar()).getByRole("button", { name }));
      expect(screen.getByRole("main").querySelector("svg[data-state]"), name).not.toBeNull();
    }
    fireEvent.click(within(sidebar()).getByRole("button", { name: "Settings" }));
    expect(screen.getByRole("main").querySelector("svg[data-state]")).toBeNull();
  });

  it("navigates with Ctrl+1…7 (⌘ on macOS)", () => {
    mockBackend();
    renderApp();
    press("3", { ctrlKey: true });
    expect(pageTitle()).toHaveTextContent("Cleanup");
    press("7", { ctrlKey: true });
    expect(pageTitle()).toHaveTextContent("Settings");
    press("1", { metaKey: true }); // ⌘ is not the modifier off macOS
    press("8", { ctrlKey: true }); // there is no eighth section
    press("2", { ctrlKey: true, altKey: true }); // AltGr on Windows is Ctrl+Alt
    expect(pageTitle()).toHaveTextContent("Settings");
  });

  it("moves focus to the page title after keyboard navigation, for screen readers", () => {
    mockBackend();
    renderApp();
    press("4", { ctrlKey: true });
    expect(pageTitle()).toHaveTextContent("Rename");
    expect(pageTitle()).toHaveFocus();
  });

  it("navigates by the physical digit key, so AZERTY keyboards work", () => {
    mockBackend();
    renderApp();
    fireEvent.keyDown(window, { key: "\"", code: "Digit3", ctrlKey: true });
    expect(pageTitle()).toHaveTextContent("Cleanup");
  });

  it("uses ⌘ as the modifier on macOS", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0)");
    mockBackend();
    renderApp();
    press("2", { metaKey: true });
    expect(pageTitle()).toHaveTextContent("Browse");
    expect(screen.getByText("⌘K")).toBeInTheDocument();
  });

  it("focuses the search field with Ctrl+K", () => {
    mockBackend();
    renderApp();
    press("k", { ctrlKey: true });
    expect(screen.getByRole("searchbox", { name: "Search files" })).toHaveFocus();
  });

  it("opens History from the top bar", () => {
    mockBackend();
    renderApp();
    fireEvent.click(screen.getByRole("button", { name: "Open History" }));
    expect(pageTitle()).toHaveTextContent("History");
  });

  it("shows the keyboard cheat-sheet with ? and closes it with Esc", async () => {
    mockBackend();
    renderApp();
    press("?");
    const sheet = await screen.findByRole("dialog", { name: "Keyboard shortcuts" });
    expect(within(sheet).getByText("Undo last batch")).toBeInTheDocument();
    expect(within(sheet).getByText("Move to Trash")).toBeInTheDocument();
    fireEvent.keyDown(sheet, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("pauses the global shortcuts while the cheat-sheet is open, and ? closes it again", async () => {
    mockBackend();
    renderApp();
    press("?");
    await screen.findByRole("dialog");
    press("3", { ctrlKey: true });
    // The page behind a modal is hidden from assistive tech, hence `hidden: true`.
    expect(screen.getByRole("heading", { level: 1, hidden: true })).toHaveTextContent("Overview");
    press("?");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("ignores a held-down ? (key repeat) and keys pressed while composing text", async () => {
    mockBackend();
    renderApp();
    press("?");
    await screen.findByRole("dialog");
    press("?", { repeat: true });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    press("?", { isComposing: true });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("doesn't take over keys while typing in a text field", () => {
    mockBackend();
    renderApp();
    const search = screen.getByRole("searchbox", { name: "Search files" });
    fireEvent.keyDown(search, { key: "?" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("renders in Dutch when the OS language is Dutch", async () => {
    mockBackend((cmd) => (cmd === "get_locale" ? "nl-BE" : BACKEND[cmd]));
    renderApp();
    expect(await within(sidebar()).findByRole("button", { name: "Overzicht" })).toBeInTheDocument();
    fireEvent.click(within(sidebar()).getByRole("button", { name: "Opruimen" }));
    expect(screen.getByRole("heading", { level: 2, name: "Geen spatje kipple." })).toBeInTheDocument();
  });

  it("changes the theme from Settings", async () => {
    const calls: unknown[] = [];
    mockBackend((cmd, args) => {
      if (cmd !== "set_theme_preference") return BACKEND[cmd];
      calls.push(args);
      return { preference: "dark", effective: "dark" };
    });
    renderApp();
    press("7", { ctrlKey: true });
    const theme = screen.getByRole("radiogroup", { name: "Theme" });
    await waitFor(() => expect(within(theme).getByRole("radio", { name: "System" })).toBeChecked());
    fireEvent.click(within(theme).getByRole("radio", { name: "Dark" }));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    expect(calls).toEqual([{ preference: "dark" }]);
  });

  it("shows the version reported by the backend in Settings", async () => {
    const calls = mockBackend();
    renderApp();
    press("7", { ctrlKey: true });
    expect(await screen.findByText("Version 1.2.3")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "About Kipple" })).toBeInTheDocument();
    await waitFor(() =>
      expect([...new Set(calls)].sort()).toEqual(["app_info", "get_appearance", "get_locale"]),
    );
  });

  it("keeps working when the backend is unavailable", () => {
    mockBackend(() => {
      throw new Error("not allowed by ACL");
    });
    renderApp();
    press("7", { ctrlKey: true });
    expect(pageTitle()).toHaveTextContent("Settings");
    expect(screen.queryByText(/^Version/)).not.toBeInTheDocument();
  });
});
