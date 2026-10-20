// SPDX-License-Identifier: GPL-3.0-or-later
import { createIntl } from "react-intl";
import { describe, expect, it } from "vitest";

import { catalogs } from "./catalog";
import { formatBytes, formatFiles, trashName } from "./format";
import { pickLocale } from "./locale";
import { pseudoLocalize } from "./pseudo";

const intl = (locale: "en" | "nl") => createIntl({ locale, messages: catalogs[locale] });

describe("pickLocale", () => {
  it("chooses Dutch for any Dutch region and English otherwise", () => {
    expect(pickLocale("nl-BE")).toBe("nl");
    expect(pickLocale("nl_NL")).toBe("nl");
    expect(pickLocale("nl")).toBe("nl");
    expect(pickLocale("fr-BE")).toBe("en");
    expect(pickLocale("en-US")).toBe("en");
    expect(pickLocale(null)).toBe("en");
  });
});

describe("formatting per language", () => {
  it("formats sizes with decimal units and the local decimal separator", () => {
    expect(formatBytes(intl("en"), 4_300_000_000)).toBe("4.3 GB");
    expect(formatBytes(intl("nl"), 4_300_000_000)).toBe("4,3 GB");
    expect(formatBytes(intl("en"), 52_000_000)).toBe("52 MB");
    expect(formatBytes(intl("en"), 0)).toBe("0 B");
    expect(formatBytes(intl("en"), 999)).toBe("999 B");
    expect(formatBytes(intl("en"), 999_960)).toBe("1 MB");
    expect(formatBytes(intl("en"), 999_999_999)).toBe("1 GB");
  });

  it("pluralises file counts", () => {
    expect(formatFiles(intl("en"), 1)).toBe("1 file");
    expect(formatFiles(intl("en"), 1284)).toBe("1,284 files");
    expect(formatFiles(intl("nl"), 1)).toBe("1 bestand");
    expect(formatFiles(intl("nl"), 1284)).toBe("1.284 bestanden");
  });

  it("names the trash the way each OS does", () => {
    expect(trashName(intl("en"), "macos")).toBe("Trash");
    expect(trashName(intl("en"), "windows")).toBe("Recycle Bin");
    expect(trashName(intl("en"), "linux")).toBe("Trash");
    expect(trashName(intl("nl"), "macos")).toBe("Prullenmand");
    expect(trashName(intl("nl"), "windows")).toBe("Prullenbak");
    expect(trashName(intl("nl"), "linux")).toBe("Prullenbak");
  });
});

describe("pseudoLocalize", () => {
  it("marks, accents and lengthens text so untranslated or clipped strings stand out", () => {
    const out = pseudoLocalize("Open Kipple");
    expect(out).toMatch(/^\[.*\]$/);
    expect(out).not.toContain("Open");
    expect(out.length).toBeGreaterThan("Open Kipple".length * 1.3);
  });

  it("pads plurals by their longest branch, not all branches together", () => {
    const out = pseudoLocalize("{count, plural, one {# file} other {# files}}");
    expect(out.match(/~/g)?.length).toBe(Math.ceil(" files".length * 0.4));
  });

  it("keeps placeholders and plural structure working", () => {
    const pseudo = createIntl({ locale: "en", messages: { m: pseudoLocalize("{count, plural, one {# file} other {# files}}") } });
    const one = pseudo.formatMessage({ id: "m" }, { count: 1 });
    const many = pseudo.formatMessage({ id: "m" }, { count: 3 });
    expect(one).toContain("1");
    expect(many).toContain("3");
    expect(one).not.toBe(many.replace("3", "1"));
  });
});
