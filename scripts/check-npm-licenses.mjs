#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Checks that every npm package in the lockfile has a GPL-3.0-compatible licence. Dev tools are
// included: some of their code ends up in the app (Tailwind's base CSS, Vite's runtime helpers).
// The allowlist is the one in deny.toml (cargo-deny, for Rust), so both sides share it.
//
//   node scripts/check-npm-licenses.mjs
import { execSync } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

// npm-only additions, with the reason:
// - OFL-1.1: the bundled fonts; font files are a separate work, not linked code.
// - BlueOak-1.0.0: a permissive licence (like MIT), used by a build tool.
const NPM_EXTRA = ["OFL-1.1", "BlueOak-1.0.0"];

/** The `allow = [...]` list from deny.toml's [licenses] section. Throws if there is none, so a
 * changed deny.toml can't silently turn this check off. */
export function allowedLicenses(denyToml) {
  // Drop comments first: a "licence" quoted in a comment must not count as allowed.
  const toml = denyToml.replace(/#.*$/gm, "");
  const section = toml.split(/^\[licenses\]\s*$/m)[1]?.split(/^\[/m)[0] ?? "";
  const list = section.match(/^allow\s*=\s*\[([\s\S]*?)\]/m)?.[1] ?? "";
  const allow = new Set([...list.matchAll(/"([^"]+)"|'([^']+)'/g)].map((m) => m[1] ?? m[2]));
  if (allow.size === 0) throw new Error("deny.toml: no licence allowlist found in [licenses]");
  return allow;
}

/** Whether an SPDX expression is acceptable: `OR` needs one allowed side, `AND` needs both. */
export function isAllowed(expression, allow) {
  const expr = expression.trim();
  if (!expr) return false;
  const parts = splitTopLevel(expr, " OR ");
  if (parts.length > 1) return parts.some((p) => isAllowed(p, allow));
  const all = splitTopLevel(expr, " AND ");
  if (all.length > 1) return all.every((p) => isAllowed(p, allow));
  if (expr.startsWith("(") && expr.endsWith(")")) return isAllowed(expr.slice(1, -1), allow);
  return allow.has(expr); // a single licence, or "X WITH exception" as listed
}

/** Splits on `separator` outside parentheses. */
function splitTopLevel(expr, separator) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < expr.length; i++) {
    if (expr[i] === "(") depth++;
    else if (expr[i] === ")") depth--;
    else if (depth === 0 && expr.startsWith(separator, i)) {
      parts.push(expr.slice(start, i));
      start = i + separator.length;
    }
  }
  return [...parts, expr.slice(start)];
}

/** `name@version: licence` for every package in a `pnpm licenses list --json` report that fails. */
export function violations(report, allow) {
  return Object.entries(report)
    .filter(([licence]) => !isAllowed(licence, allow))
    .flatMap(([licence, packages]) => packages.map((p) => `${p.name}@${p.versions.join(", ")}: ${licence}`));
}

// Run as a script (not imported by the tests). realpath: argv[1] may go through a symlink.
if (realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const allow = allowedLicenses(readFileSync(`${root}/deny.toml`, "utf8"));
  for (const extra of NPM_EXTRA) allow.add(extra);
  // One fixed command string: the shell finds pnpm's .cmd shim on Windows too.
  const report = JSON.parse(execSync("pnpm -r licenses list --json", { cwd: root, encoding: "utf8" }));
  const bad = violations(report, allow);
  if (bad.length) {
    console.error(`npm packages with a licence not allowed in deny.toml:\n  ${bad.join("\n  ")}`);
    process.exit(1);
  }
  console.log(`npm licences OK (${Object.values(report).flat().length} package(s) checked).`);
}
