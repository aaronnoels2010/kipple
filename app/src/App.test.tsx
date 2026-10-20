// SPDX-License-Identifier: GPL-3.0-or-later
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import App from "./App";
import { resetAppearanceStore } from "./theme/useAppearance";

const BACKEND: Record<string, unknown> = {
  app_info: { version: "1.2.3", os: "macos" },
  get_appearance: { preference: "system", effective: "light" },
  get_locale: "en-US",
};

// Mocks Tauri's IPC layer, so the generated bindings (command names, arguments) are exercised too.
function mockBackend(handler: (cmd: string) => unknown = (cmd) => BACKEND[cmd]) {
  const calls: string[] = [];
  mockIPC(
    (cmd) => {
      calls.push(cmd);
      return handler(cmd);
    },
    { shouldMockEvents: true },
  );
  return calls;
}

describe("App shell", () => {
  afterEach(() => {
    resetAppearanceStore();
    clearMocks();
  });

  it("shows the Kipple wordmark as the main heading", () => {
    mockBackend();
    render(<App />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveAccessibleName("kipple");
  });

  it("shows the version reported by the backend over typed IPC", async () => {
    const calls = mockBackend();
    render(<App />);
    expect(await screen.findByText("v1.2.3")).toBeInTheDocument();
    await waitFor(() => expect([...new Set(calls)].sort()).toEqual(["app_info", "get_appearance"]));
  });

  it("shows no version and doesn't crash when the backend call fails", async () => {
    mockBackend(() => {
      throw new Error("not allowed by ACL");
    });
    render(<App />);
    expect(screen.getByRole("heading", { level: 1 })).toBeInTheDocument();
    expect(screen.queryByText(/^v\d/)).not.toBeInTheDocument();
  });
});
