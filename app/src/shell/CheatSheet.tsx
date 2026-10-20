// SPDX-License-Identifier: GPL-3.0-or-later
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { FormattedMessage, useIntl } from "react-intl";

import { IconButton } from "../components/IconButton";
import { Kbd } from "../components/Kbd";
import { currentOs, shortcutLabel } from "./platform";
import { SECTIONS } from "./sections";

/** The keyboard map from DESIGN.md §11, opened with `?`. */
export function CheatSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string>) => intl.formatMessage({ id }, values);
  const os = currentOs();
  const rows: [keys: string[], action: string][] = [
    [[shortcutLabel("K")], t("shortcuts.search")],
    [[t("key.space")], t("shortcuts.quickLook")],
    [["Enter", "F2"], t("shortcuts.rename")],
    [[shortcutLabel("A")], t("shortcuts.selectAll")],
    [[os === "macos" ? "⌫" : t("key.delete")], t("shortcuts.trash", { os })],
    [["K"], t("shortcuts.keep")],
    [["P"], t("shortcuts.pin")],
    [["T"], t("shortcuts.triage")],
    [[shortcutLabel("Z")], t("shortcuts.undo")],
    [[shortcutLabel(`1…${SECTIONS.length}`)], t("shortcuts.sections")],
    [["Esc"], t("shortcuts.dismiss")],
    [["?"], t("shortcuts.help")],
  ];

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-bg/70 backdrop-blur-sm" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed top-1/2 left-1/2 max-h-[85vh] w-[min(440px,calc(100vw-32px))] -translate-x-1/2 -translate-y-1/2 overflow-auto rounded-xl border border-border bg-surface p-[22px] text-text shadow-3"
        >
          <div className="mb-4 flex items-start justify-between gap-4">
            <Dialog.Title className="font-display text-xl font-semibold font-soft">
              <FormattedMessage id="shortcuts.title" />
            </Dialog.Title>
            <Dialog.Close asChild>
              <IconButton label={t("shortcuts.close")} icon={X} className="text-muted hover:text-text" />
            </Dialog.Close>
          </div>
          <dl className="grid gap-2 text-[13px]">
            {rows.map(([keys, action]) => (
              <div key={action} className="flex items-center justify-between gap-3">
                <dt>{action}</dt>
                <dd className="flex shrink-0 gap-1">
                  {keys.map((key) => (
                    <Kbd key={key}>{key}</Kbd>
                  ))}
                </dd>
              </div>
            ))}
          </dl>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
