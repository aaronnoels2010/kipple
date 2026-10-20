// SPDX-License-Identifier: GPL-3.0-or-later
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { Appearance, ThemePreference } from "../bindings";
import { resetAppearanceStore, useAppearance } from "./useAppearance";

describe("useAppearance", () => {
  let backend: Appearance;
  let saved: ThemePreference[];
  let failSave: boolean;

  beforeEach(() => {
    backend = { preference: "system", effective: "dark" };
    saved = [];
    failSave = false;
    mockIPC(
      (cmd, args) => {
        if (cmd === "get_appearance") return backend;
        if (cmd === "set_theme_preference") {
          if (failSave) throw { code: "settings_save_failed", params: {} };
          const preference = (args as { preference: ThemePreference }).preference;
          saved.push(preference);
          backend = { preference, effective: preference === "system" ? backend.effective : preference };
          return backend;
        }
      },
      { shouldMockEvents: true },
    );
  });

  afterEach(() => {
    resetAppearanceStore();
    clearMocks();
    delete document.documentElement.dataset.theme;
  });

  it("applies the effective theme from the backend to <html data-theme>", async () => {
    renderHook(() => useAppearance());
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
  });

  it("follows the OS when the backend reports a change", async () => {
    renderHook(() => useAppearance());
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    await act(() => emit("appearance-changed", { appearance: { preference: "system", effective: "light" } }));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
  });

  it("saves an override through the backend and every consumer sees it", async () => {
    const first = renderHook(() => useAppearance());
    const second = renderHook(() => useAppearance());
    await waitFor(() => expect(first.result.current.appearance).toBeDefined());
    await act(() => first.result.current.setPreference("light"));
    expect(saved).toEqual(["light"]);
    expect(second.result.current.appearance?.preference).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("leaves the theme unchanged when saving fails, and reports the error code", async () => {
    const { result } = renderHook(() => useAppearance());
    await waitFor(() => expect(result.current.appearance).toBeDefined());
    failSave = true;
    let outcome: Awaited<ReturnType<typeof result.current.setPreference>> | undefined;
    await act(async () => {
      outcome = await result.current.setPreference("light");
    });
    expect(outcome).toEqual({ status: "error", error: { code: "settings_save_failed", params: {} } });
    expect(result.current.appearance?.preference).toBe("system");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
