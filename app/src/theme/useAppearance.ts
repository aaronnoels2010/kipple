// SPDX-License-Identifier: GPL-3.0-or-later
import { useSyncExternalStore } from "react";

import { type Appearance, commands, events, type ThemePreference } from "../bindings";

// One app-wide store: every component sees the same appearance, and the backend is asked once.
let current: Appearance | undefined;
let started = false;
let eventSeen = false;
let stopListening: Promise<() => void> | undefined;
const subscribers = new Set<() => void>();

function publish(next: Appearance) {
  current = next;
  document.documentElement.dataset.theme = next.effective;
  for (const notify of subscribers) notify();
}

function start() {
  if (started) return;
  started = true;
  // Subscribe before fetching, and let an event win over a slower initial fetch.
  stopListening = events.appearanceChanged.listen((e) => {
    eventSeen = true;
    publish(e.payload.appearance);
  });
  stopListening.catch(() => {});
  commands
    .getAppearance()
    .then((a) => eventSeen || publish(a))
    // Outside Tauri (plain `vite dev`) there is no backend: keep the prefers-color-scheme guess.
    .catch(() => {});
}

function subscribe(notify: () => void) {
  start();
  subscribers.add(notify);
  return () => subscribers.delete(notify);
}

async function setPreference(preference: ThemePreference) {
  const result = await commands.setThemePreference(preference);
  if (result.status === "ok") publish(result.data);
  return result;
}

/** The appearance from the backend (OS theme + saved override), shared across the app. */
export function useAppearance() {
  const appearance = useSyncExternalStore(subscribe, () => current);
  return { appearance, setPreference };
}

/** Tests only: forget the shared state between test cases. */
export function resetAppearanceStore() {
  stopListening?.then((stop) => stop()).catch(() => {});
  current = undefined;
  started = false;
  eventSeen = false;
  stopListening = undefined;
  subscribers.clear();
}
