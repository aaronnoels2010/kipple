// SPDX-License-Identifier: GPL-3.0-or-later
import * as ToggleGroup from "@radix-ui/react-toggle-group";
import { Monitor, Moon, Sun } from "lucide-react";
import { type ReactNode, useEffect, useId, useState } from "react";
import { FormattedMessage } from "react-intl";

import { commands, type ThemePreference } from "../bindings";
import { useAppearance } from "../theme/useAppearance";

const THEMES = [
  { value: "light", icon: Sun, label: "settings.theme.light" },
  { value: "dark", icon: Moon, label: "settings.theme.dark" },
  { value: "system", icon: Monitor, label: "settings.theme.system" },
] as const satisfies readonly { value: ThemePreference; icon: unknown; label: string }[];

function Card({ title, children }: { title: ReactNode; children: ReactNode }) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="rounded-lg border border-border bg-surface p-4 shadow-1">
      <h2 id={id} className="mb-3 font-semibold">
        {title}
      </h2>
      {children}
    </section>
  );
}

function ThemeSwitch() {
  const labelId = useId();
  const { appearance, setPreference } = useAppearance();
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <span id={labelId}>
        <FormattedMessage id="settings.theme" />
      </span>
      <ToggleGroup.Root
        type="single"
        aria-labelledby={labelId}
        value={appearance?.preference ?? ""}
        // Radix reports "" when the active item is clicked again: keep the current choice.
        onValueChange={(value) => value && void setPreference(value as ThemePreference)}
        className="inline-flex gap-0.5 rounded-full border border-border bg-surface-2 p-[3px]"
      >
        {THEMES.map(({ value, icon: Icon, label }) => (
          <ToggleGroup.Item
            key={value}
            value={value}
            className="inline-flex h-7 items-center gap-1.5 rounded-full px-3 text-[12.5px] font-semibold text-muted hover:text-text data-[state=on]:bg-surface data-[state=on]:text-text data-[state=on]:shadow-1"
          >
            <Icon size={14} strokeWidth={1.9} />
            <FormattedMessage id={label} />
          </ToggleGroup.Item>
        ))}
      </ToggleGroup.Root>
    </div>
  );
}

function useVersion() {
  const [version, setVersion] = useState<string>();
  useEffect(() => {
    let mounted = true;
    commands
      .appInfo()
      .then((info) => mounted && setVersion(info.version))
      // Outside Tauri (plain `vite dev` in a browser) there is no backend: show no version.
      .catch(() => {});
    return () => {
      mounted = false;
    };
  }, []);
  return version;
}

export function SettingsScreen() {
  const version = useVersion();
  return (
    <div className="grid max-w-[640px] gap-4">
      <Card title={<FormattedMessage id="settings.appearance" />}>
        <ThemeSwitch />
      </Card>
      <Card title={<FormattedMessage id="settings.about" />}>
        {version && (
          <p className="font-mono text-xs text-muted">
            <FormattedMessage id="settings.version" values={{ version }} />
          </p>
        )}
      </Card>
    </div>
  );
}
