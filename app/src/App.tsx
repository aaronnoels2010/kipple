// SPDX-License-Identifier: GPL-3.0-or-later
// "kipple" is the brand wordmark and "v1.2.3" a version number: neither is translatable text.
import { useEffect, useState } from "react";

import { commands } from "./bindings";

export default function App() {
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
    <main>
      <h1>kipple</h1>
      {version && <p>v{version}</p>}
    </main>
  );
}
