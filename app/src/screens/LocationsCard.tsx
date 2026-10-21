// SPDX-License-Identifier: GPL-3.0-or-later
// Settings → Locations: the folders Kipple manages (K-2.4). Folders are only ever chosen by the
// OS (a default user folder or the native dialog, opened by Rust): this screen sends IDs and
// folder kinds, never paths (ADR-0007).
import { CloudUpload, FolderPlus, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { FormattedMessage, useIntl } from "react-intl";

import { type Coded, commands, type DefaultFolder, type LocationView } from "../bindings";
import { Card } from "../components/Card";
import { IconButton } from "../components/IconButton";
import { Switch } from "../components/Switch";

const DEFAULT_FOLDERS = [
  { folder: "desktop", label: "locations.folder.desktop" },
  { folder: "downloads", label: "locations.folder.downloads" },
  { folder: "documents", label: "locations.folder.documents" },
  { folder: "pictures", label: "locations.folder.pictures" },
  { folder: "music", label: "locations.folder.music" },
  { folder: "videos", label: "locations.folder.videos" },
] as const satisfies readonly { folder: DefaultFolder; label: string }[];

/** Backend error codes (ADR-0004) → messages. */
const ERRORS: Record<string, string> = {
  location_not_a_folder: "locations.error.notAFolder",
  location_default_missing: "locations.error.defaultMissing",
  location_unsupported_name: "locations.error.unsupportedName",
  location_protected: "locations.error.protected",
  location_already_added: "locations.error.alreadyAdded",
  location_inside_location: "locations.error.insideLocation",
  location_contains_location: "locations.error.containsLocation",
  location_unreadable: "locations.error.unreadable",
  location_storage_failed: "locations.error.storageFailed",
  location_not_found: "locations.error.notFound",
  location_dialog_failed: "locations.error.dialogFailed",
};

/** The last part of a path, for display: "/Volumes/USB/Photos" → "Photos". */
function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

type Result = { status: "ok" } | { status: "error"; error: Coded };
const UNKNOWN: Coded = { code: "unknown", params: {} };

function useLocations() {
  const [locations, setLocations] = useState<LocationView[]>();
  const [error, setError] = useState<Coded>();
  const [busy, setBusy] = useState(false);
  const reload = useCallback(async () => {
    const result = await commands.locationsList().catch(() => ({ status: "error" as const, error: UNKNOWN }));
    if (result.status === "ok") setLocations(result.data);
    else setError(result.error);
  }, []);
  useEffect(() => {
    void reload();
  }, [reload]);
  /** Runs one change (one at a time), shows its error if any, and reloads the list. */
  const run = useCallback(
    async (change: () => Promise<Result>) => {
      setBusy(true);
      setError(undefined);
      const result = await change().catch((): Result => ({ status: "error", error: UNKNOWN }));
      if (result.status === "error") setError(result.error);
      await reload();
      setBusy(false);
    },
    [reload],
  );
  return { locations, error, busy, run, retry: () => run(async () => ({ status: "ok" })) };
}

function ErrorMessage({ error }: { error: Coded }) {
  return (
    <p role="alert" className="mt-3 rounded-md bg-comb-soft px-3 py-2 text-comb">
      <FormattedMessage id={ERRORS[error.code] ?? "locations.error.unknown"} />
    </p>
  );
}

const BUTTON =
  "mt-3 inline-flex h-[34px] items-center gap-2 rounded-md border border-border bg-surface px-3.5 font-semibold shadow-1 hover:bg-surface-2 disabled:opacity-45";

export function LocationsCard() {
  const intl = useIntl();
  const { locations, error, busy, run, retry } = useLocations();
  if (!locations) {
    // The list couldn't be loaded: say so and offer a retry, instead of an empty card.
    return error ? (
      <Card title={<FormattedMessage id="settings.locations" />}>
        <ErrorMessage error={error} />
        <button type="button" onClick={retry} disabled={busy} className={BUTTON}>
          <FormattedMessage id="locations.retry" />
        </button>
      </Card>
    ) : null;
  }

  const byKind = (folder: DefaultFolder) => locations.find((l) => l.kind === folder);
  const others = locations.filter((l) => !DEFAULT_FOLDERS.some((d) => d.folder === l.kind));

  return (
    <Card title={<FormattedMessage id="settings.locations" />}>
      <p className="mb-3 text-muted">
        <FormattedMessage id="locations.intro" />
      </p>
      <ul className="grid gap-1">
        {DEFAULT_FOLDERS.map(({ folder, label }) => {
          const location = byKind(folder);
          const name = intl.formatMessage({ id: label });
          return (
            <li key={folder} className="flex min-h-9 items-center justify-between gap-3">
              <span>{name}</span>
              <Switch
                label={name}
                disabled={busy}
                checked={location?.enabled ?? false}
                onChange={(on) =>
                  run(() =>
                    location
                      ? commands.locationsSetEnabled(location.id, on)
                      : commands.locationsAddDefault(folder),
                  )
                }
              />
            </li>
          );
        })}
        {others.map((location) => {
          const name = folderName(location.path);
          return (
            <li key={location.id} className="flex min-h-9 items-center justify-between gap-3">
              <span className="grid min-w-0">
                <span className="flex flex-wrap items-center gap-2">
                  <span>{name}</span>
                  {!location.online && (
                    <span className="rounded-full bg-surface-2 px-2 text-xs text-muted">
                      <FormattedMessage id="locations.offline" />
                    </span>
                  )}
                  {location.in_sync_root && (
                    <span className="inline-flex items-center gap-1 rounded-full bg-sky-soft px-2 text-xs text-sky">
                      <CloudUpload size={12} strokeWidth={1.9} aria-hidden="true" />
                      <FormattedMessage id="locations.syncRoot" />
                    </span>
                  )}
                </span>
                <span className="truncate font-mono text-xs text-muted" title={location.path}>
                  {location.path}
                </span>
              </span>
              <span className="flex shrink-0 items-center gap-2">
                <Switch
                  label={name}
                  disabled={busy}
                  checked={location.enabled}
                  onChange={(on) => run(() => commands.locationsSetEnabled(location.id, on))}
                />
                <IconButton
                  label={intl.formatMessage({ id: "locations.remove" }, { name })}
                  icon={X}
                  className="text-muted hover:text-text"
                  disabled={busy}
                  onClick={() => run(() => commands.locationsRemove(location.id))}
                />
              </span>
            </li>
          );
        })}
      </ul>
      {error && <ErrorMessage error={error} />}
      <button
        type="button"
        disabled={busy}
        onClick={() => run(() => commands.locationsAddViaDialog())}
        className={BUTTON}
      >
        <FolderPlus size={16} strokeWidth={1.9} aria-hidden="true" />
        <FormattedMessage id="locations.add" />
      </button>
    </Card>
  );
}
