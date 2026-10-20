// SPDX-License-Identifier: GPL-3.0-or-later
// Pseudo-locale: every literal is accented, lengthened and bracketed, so text that skipped the
// catalog (still plain) or gets clipped (missing "]") stands out. ICU syntax is preserved.
import { type MessageFormatElement, parse, TYPE } from "@formatjs/icu-messageformat-parser";
import { printAST } from "@formatjs/icu-messageformat-parser/printer.js";

const ACCENTS: Record<string, string> = {
  a: "á", e: "é", i: "í", o: "ö", u: "ú", y: "ý", c: "ç", n: "ñ", s: "š", d: "ď", t: "ŧ", l: "ļ", f: "ƒ",
  A: "Å", E: "É", I: "Î", O: "Ö", U: "Û", Y: "Ý", C: "Ç", N: "Ñ", S: "Š", D: "Ď", T: "Ŧ", K: "Ķ",
};

/** Roughly how much longer translations can be than English (DESIGN.md §7: +35%). */
const EXPANSION = 0.4;

function transform(elements: MessageFormatElement[]): number {
  let letters = 0;
  for (const el of elements) {
    if (el.type === TYPE.literal) {
      letters += el.value.length;
      el.value = [...el.value].map((ch) => ACCENTS[ch] ?? ch).join("");
    } else if (el.type === TYPE.plural || el.type === TYPE.select) {
      letters += Math.max(...Object.values(el.options).map((option) => transform(option.value)));
    } else if (el.type === TYPE.tag) {
      letters += transform(el.children);
    }
  }
  return letters;
}

export function pseudoLocalize(message: string): string {
  const ast = parse(message);
  const letters = transform(ast);
  const padding = "~".repeat(Math.ceil(letters * EXPANSION));
  return `[${printAST(ast)}${padding}]`;
}
