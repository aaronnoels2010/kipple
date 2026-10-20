// SPDX-License-Identifier: GPL-3.0-or-later
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { getNonce } from "get-nonce";
import { afterEach, describe, expect, it } from "vitest";

import { adoptStyleNonce } from "./styleNonce";

describe("adoptStyleNonce", () => {
  afterEach(() => document.head.replaceChildren());

  it("hands the nonce Tauri put on index.html's <style> to runtime-injected styles", () => {
    const style = document.createElement("style");
    style.nonce = "abc123";
    document.head.append(style);
    adoptStyleNonce(document);
    expect(getNonce()).toBe("abc123");
  });

  it("has the <style> in index.html that Tauri puts the nonce on", () => {
    const html = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../index.html"), "utf8");
    expect(html).toMatch(/<head>[\s\S]*<style><\/style>[\s\S]*<\/head>/);
  });

  it("does nothing without a nonce (dev server, plain browser)", () => {
    document.head.append(document.createElement("style"));
    expect(() => adoptStyleNonce(document)).not.toThrow();
  });
});
