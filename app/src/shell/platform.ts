// SPDX-License-Identifier: GPL-3.0-or-later
// Read from the webview's user agent: synchronous, so the first render already has the right keys.

export type Os = "macos" | "windows" | "linux";

export function currentOs(): Os {
  const ua = navigator.userAgent;
  if (/Mac/.test(ua)) return "macos";
  if (/Windows/.test(ua)) return "windows";
  return "linux";
}

/** ⌘ on macOS, Ctrl elsewhere. */
export function hasModifier(e: KeyboardEvent): boolean {
  return currentOs() === "macos" ? e.metaKey : e.ctrlKey;
}

/** How a modifier shortcut is written on this OS: `⌘K` or `Ctrl+K`. */
export function shortcutLabel(key: string): string {
  return currentOs() === "macos" ? `⌘${key}` : `Ctrl+${key}`;
}
