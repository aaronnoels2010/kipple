// SPDX-License-Identifier: GPL-3.0-or-later
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import App from "./App";

// Mocks Tauri's IPC layer, so the generated bindings (command name, arguments) are exercised too.
function mockBackend(handler: (cmd: string) => unknown) {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return handler(cmd);
  });
  return calls;
}

describe("App shell", () => {
  afterEach(() => clearMocks());

  it("shows the Kipple wordmark as the main heading", () => {
    mockBackend(() => ({ version: "1.2.3", os: "macos" }));
    render(<App />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveAccessibleName("kipple");
  });

  it("shows the version reported by the backend over typed IPC", async () => {
    const calls = mockBackend((cmd) => (cmd === "app_info" ? { version: "1.2.3", os: "macos" } : undefined));
    render(<App />);
    expect(await screen.findByText("v1.2.3")).toBeInTheDocument();
    expect(calls).toEqual(["app_info"]);
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
