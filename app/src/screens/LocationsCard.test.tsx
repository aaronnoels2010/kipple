// SPDX-License-Identifier: GPL-3.0-or-later
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { LocationView } from "../bindings";
import { I18nProvider } from "../i18n/I18nProvider";
import { LocationsCard } from "./LocationsCard";

const downloads: LocationView = {
  id: 1,
  kind: "downloads",
  path: "/Users/zoe/Downloads",
  enabled: true,
  online: true,
  in_sync_root: false,
  can_trash: true,
};
const usb: LocationView = {
  id: 2,
  kind: "custom",
  path: "/Volumes/USB/Photos",
  enabled: true,
  online: false,
  in_sync_root: true,
  can_trash: false,
};

/** A fake backend: `locations` is what locations_list returns; every call is recorded. */
function backend(locations: LocationView[], overrides: Record<string, (args: any) => unknown> = {}) {
  const calls: [string, unknown][] = [];
  mockIPC((cmd, args) => {
    calls.push([cmd, args]);
    if (cmd in overrides) return overrides[cmd]!(args);
    if (cmd === "get_locale") return "en-US";
    if (cmd === "locations_list") return locations;
    if (cmd === "locations_add_default") return { ...downloads, id: 9, kind: (args as any).folder };
    if (cmd === "locations_set_enabled") return { ...downloads, enabled: (args as any).enabled };
    return null;
  });
  return calls;
}

function renderCard() {
  render(
    <I18nProvider>
      <LocationsCard />
    </I18nProvider>,
  );
}

const switchFor = (name: string) => screen.getByRole("switch", { name });
const called = (calls: [string, unknown][], cmd: string) => calls.filter(([c]) => c === cmd).map(([, a]) => a);

describe("LocationsCard", () => {
  afterEach(() => clearMocks());

  it("shows every default folder with a switch, on for the ones Kipple manages", async () => {
    backend([downloads, usb]);
    renderCard();
    expect(await screen.findByRole("switch", { name: "Downloads" })).toBeChecked();
    for (const name of ["Desktop", "Documents", "Pictures", "Music", "Videos"]) {
      expect(switchFor(name)).not.toBeChecked();
    }
  });

  it("lists added folders with their state", async () => {
    backend([downloads, usb]);
    renderCard();
    const row = (await screen.findByText("Photos")).closest("li")!;
    expect(within(row).getByText("/Volumes/USB/Photos")).toBeInTheDocument();
    expect(within(row).getByText("Offline")).toBeInTheDocument();
    expect(within(row).getByText("Synced to the cloud")).toBeInTheDocument();
    expect(within(row).getByRole("switch", { name: "Photos" })).toBeChecked();
  });

  it("adds a default folder, or pauses one that's on", async () => {
    const calls = backend([downloads]);
    renderCard();
    fireEvent.click(await screen.findByRole("switch", { name: "Desktop" }));
    await waitFor(() => expect(called(calls, "locations_add_default")).toEqual([{ folder: "desktop" }]));
    fireEvent.click(switchFor("Downloads"));
    await waitFor(() => expect(called(calls, "locations_set_enabled")).toEqual([{ id: 1, enabled: false }]));
  });

  it("adds a folder through the native dialog and stops managing one", async () => {
    const calls = backend([downloads, usb]);
    renderCard();
    fireEvent.click(await screen.findByRole("button", { name: "Add folder…" }));
    await waitFor(() => expect(called(calls, "locations_add_via_dialog")).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Stop managing Photos" }));
    await waitFor(() => expect(called(calls, "locations_remove")).toEqual([{ id: 2 }]));
  });

  it("keeps the card, with a retry, when the list can't be loaded", async () => {
    let fail = true;
    const calls = backend([], {
      locations_list: () => {
        if (fail) throw { code: "location_storage_failed", params: {} };
        return [downloads];
      },
    });
    renderCard();
    expect(await screen.findByRole("alert")).toHaveTextContent("couldn't save");
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("switch", { name: "Downloads" })).toBeChecked();
    expect(called(calls, "locations_list")).toHaveLength(2);
  });

  it("explains why a folder can't be added, from its code", async () => {
    backend([], {
      locations_add_via_dialog: () => {
        throw { code: "location_protected", params: {} };
      },
    });
    renderCard();
    fireEvent.click(await screen.findByRole("button", { name: "Add folder…" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("system or protected folder");
  });
});
