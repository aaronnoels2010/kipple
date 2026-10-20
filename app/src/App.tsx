// SPDX-License-Identifier: GPL-3.0-or-later
// "kipple" is the brand wordmark and "v1.2.3" a version number: neither is translatable text.
import { useEffect, useState } from "react";

import { commands } from "./bindings";
import { Hen } from "./components/Hen";
import { useAppearance } from "./theme/useAppearance";

export default function App() {
  useAppearance();
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

  return (
    <main className="grid min-h-screen place-content-center justify-items-center gap-3 bg-bg text-text">
      <Hen state="idle" className="size-28" />
      <h1 className="font-display text-5xl font-bold tracking-tight">kipple</h1>
      {version && <p className="font-mono text-xs text-muted">v{version}</p>}
    </main>
  );
}
