// SPDX-License-Identifier: GPL-3.0-or-later
// The catalog rules from ADR-0004 / locales/README.md, enforced in CI.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { parse, TYPE, type MessageFormatElement } from "@formatjs/icu-messageformat-parser";
import { describe, expect, it } from "vitest";

import enApp from "../../../locales/en/app.json";
import enTray from "../../../locales/en/tray.json";
import { catalogs, SOURCE_LOCALE } from "./catalog";

/** Placeholder names with their kind, e.g. `count:plural`, including nested ones. */
function placeholders(elements: MessageFormatElement[], out = new Set<string>()): Set<string> {
  for (const el of elements) {
    if (el.type === TYPE.literal || el.type === TYPE.pound) continue;
    if ("value" in el && typeof el.value === "string") out.add(`${el.value}:${TYPE[el.type]}`);
    if ("options" in el) for (const option of Object.values(el.options)) placeholders(option.value, out);
    if (el.type === TYPE.tag) placeholders(el.children, out);
  }
  return out;
}

const source = catalogs[SOURCE_LOCALE];
const others = Object.entries(catalogs).filter(([locale]) => locale !== SOURCE_LOCALE);

describe("message catalogs", () => {
  it.each(others)("%s has exactly the same keys as the source language", (_, messages) => {
    expect(Object.keys(messages).sort()).toEqual(Object.keys(source).sort());
  });

  it("keeps UI and tray keys apart (they are merged into one catalog)", () => {
    expect(Object.keys(enApp).filter((k) => k in enTray)).toEqual([]);
  });

  it.each(Object.entries(catalogs))("every %s message is valid ICU MessageFormat", (_, messages) => {
    for (const [key, message] of Object.entries(messages)) {
      expect(() => parse(message), key).not.toThrow();
    }
  });

  it.each(others)("%s uses the same placeholders as the source language", (_, messages) => {
    for (const [key, message] of Object.entries(source)) {
      expect([...placeholders(parse(messages[key]!))].sort(), key).toEqual([...placeholders(parse(message))].sort());
    }
  });
});

describe("message keys in the code", () => {
  // Every string literal that looks like a catalog key ("area.name"), in non-test source files.
  // A string, not `new URL(...)`: under jsdom the global URL isn't Node's, which fs rejects.
  const srcDir = join(dirname(fileURLToPath(import.meta.url)), "..");
  const files = readdirSync(srcDir, { recursive: true, encoding: "utf8" }).filter(
    (f) => /\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f) && !f.startsWith("bindings"),
  );
  const used = new Set(
    files.flatMap((f) =>
      [...readFileSync(join(srcDir, f), "utf8").matchAll(/["']([a-z]+(?:\.[a-zA-Z0-9]+)+)["']/g)].map((m) => m[1]!),
    ),
  );
  const uiKeys = Object.keys(enApp); // tray.json is used by Rust

  it("uses no key that is missing from the catalog", () => {
    const keyLike = [...used].filter((k) => uiKeys.some((u) => u.split(".")[0] === k.split(".")[0]));
    expect(keyLike.filter((k) => !(k in source))).toEqual([]);
  });

  it("has no unused UI keys", () => {
    expect(uiKeys.filter((k) => !used.has(k))).toEqual([]);
  });
});
