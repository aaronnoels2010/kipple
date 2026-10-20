// SPDX-License-Identifier: GPL-3.0-or-later
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

// Read from disk: Vitest replaces CSS imports (even ?raw) with empty strings.
const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");
const appCss = read("./app.css");
const tokensCss = read("./tokens.css");
const designHtml = read("../../../docs/design.html");

/** Custom properties declared in the first `selector { … }` block, as name → value. */
function block(css: string, selector: string): Map<string, string> {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`missing ${selector}`);
  const body = css.slice(start, css.indexOf("}", start));
  return new Map([...body.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((m) => [m[1]!, m[2]!.trim()]));
}

describe("design tokens", () => {
  const light = block(tokensCss, ":root");
  const dark = block(tokensCss, ':root[data-theme="dark"]');

  it("defines the same tokens for light and dark", () => {
    expect([...dark.keys()].sort()).toEqual([...light.keys()].sort());
    expect(light.size).toBeGreaterThan(40);
  });

  it("matches the reviewed values in docs/design.html exactly", () => {
    expect(light).toEqual(block(designHtml, ":root, .theme-light"));
    expect(dark).toEqual(block(designHtml, '.theme-dark, :root[data-theme="dark"]'));
  });

  it("only maps tokens that exist into the Tailwind theme", () => {
    const used = [...appCss.matchAll(/var\((--[\w-]+)\)/g)].map((m) => m[1]!);
    expect(used.length).toBeGreaterThan(10);
    expect(used.filter((t) => !light.has(t))).toEqual([]);
  });

  it("never uses raw colours outside tokens.css", () => {
    expect(appCss).not.toMatch(/#[0-9a-fA-F]{3,8}\b|rgb\(|hsl\(/);
  });
});
