// SPDX-License-Identifier: GPL-3.0-or-later

/** An on/off switch (DESIGN.md §8): sage when on. Its accessible name comes from `label`; the
 * clickable area is 28 px high around a smaller track (DESIGN.md §11, hit targets). */
export function Switch({
  checked,
  label,
  disabled = false,
  onChange,
}: {
  checked: boolean;
  label: string;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="inline-flex h-7 w-11 shrink-0 items-center justify-center rounded-full disabled:opacity-45"
    >
      <span
        aria-hidden="true"
        className={`inline-flex h-5 w-9 items-center rounded-full transition-colors duration-200 ${
          checked ? "bg-sage" : "bg-surface-3"
        }`}
      >
        <span
          className={`size-4 rounded-full bg-surface shadow-1 transition-transform duration-200 ${
            checked ? "translate-x-[18px]" : "translate-x-[2px]"
          }`}
        />
      </span>
    </button>
  );
}
