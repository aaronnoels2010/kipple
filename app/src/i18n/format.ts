// SPDX-License-Identifier: GPL-3.0-or-later
import type { IntlShape } from "react-intl";

// SI symbols, the same in every language we ship.
const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** Bytes are whole; larger units get one decimal. */
const rounded = (value: number, unit: number) => (unit === 0 ? Math.round(value) : Math.round(value * 10) / 10);

/** Decimal (1000-based) sizes with the language's decimal separator: `4.3 GB` / `4,3 GB`. */
export function formatBytes(intl: IntlShape, bytes: number): string {
  let value = bytes;
  let unit = 0;
  // Compare the rounded value, so 999,960 bytes becomes "1 MB", not "1,000 KB".
  while (rounded(value, unit) >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  const number = intl.formatNumber(value, { maximumFractionDigits: unit === 0 ? 0 : 1 });
  return `${number} ${UNITS[unit]}`;
}

export function formatFiles(intl: IntlShape, count: number): string {
  return intl.formatMessage({ id: "common.files" }, { count });
}

/** What the user's OS calls its trash ("Recycle Bin", "Prullenmand", …). */
export function trashName(intl: IntlShape, os: string): string {
  return intl.formatMessage({ id: "trash.name" }, { os });
}
