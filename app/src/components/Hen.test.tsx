// SPDX-License-Identifier: GPL-3.0-or-later
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Hen, type HenState } from "./Hen";

const STATES: HenState[] = ["idle", "happy", "sleeping", "alert", "nesting"];

describe("Hen", () => {
  it.each(STATES)("renders the %s mood as decorative SVG", (state) => {
    const { container } = render(<Hen state={state} />);
    const svg = container.querySelector("svg")!;
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("data-state", state);
  });

  it("gives each mood a different face", () => {
    const faces = STATES.map((state) => render(<Hen state={state} />).container.innerHTML);
    expect(new Set(faces).size).toBe(STATES.length);
  });

  it("uses theme tokens for its colours, never raw values", () => {
    const { container } = render(<Hen state="happy" />);
    const fills = [...container.querySelectorAll("[fill],[stroke]")].flatMap((el) =>
      [el.getAttribute("fill"), el.getAttribute("stroke")].filter((v) => v && v !== "none"),
    );
    expect(fills.length).toBeGreaterThan(5);
    expect(fills.filter((v) => !v!.startsWith("var(--"))).toEqual([]);
  });
});
