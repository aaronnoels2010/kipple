// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import App from "./App";

describe("App shell", () => {
  it("shows the Kipple wordmark as the main heading", () => {
    render(<App />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveAccessibleName("kipple");
  });
});
