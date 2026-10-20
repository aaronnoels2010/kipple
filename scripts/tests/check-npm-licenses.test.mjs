// SPDX-License-Identifier: GPL-3.0-or-later
// Tests for scripts/check-npm-licenses.mjs. Run: node --test "scripts/tests/*.test.mjs"
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { allowedLicenses, isAllowed, violations } from "../check-npm-licenses.mjs";

const allow = new Set(["MIT", "Apache-2.0", "BSD-3-Clause", "Apache-2.0 WITH LLVM-exception"]);

test("reads the allowlist from deny.toml, so Rust and npm share one list", () => {
  const list = allowedLicenses(readFileSync(new URL("../../deny.toml", import.meta.url), "utf8"));
  assert.ok(list.has("MIT") && list.has("GPL-3.0-or-later") && list.has("Apache-2.0 WITH LLVM-exception"));
  assert.ok(!list.has("GPL-2.0-only"));
});

test("ignores TOML comments in the allowlist and refuses an empty one", () => {
  const toml = `[licenses] # comment
allow = [
  "MIT", # see [notes]
  # never add "GPL-2.0-only" here
  'ISC',
]
[bans]
allow = ["Other"]
`;
  assert.deepEqual([...allowedLicenses(toml)].sort(), ["ISC", "MIT"]);
  assert.throws(() => allowedLicenses("[licenses]\nallow = []\n"), /no licence allowlist/);
  assert.throws(() => allowedLicenses("[bans]\n"), /no licence allowlist/);
});

test("evaluates SPDX expressions", () => {
  assert.ok(isAllowed("MIT", allow));
  assert.ok(isAllowed("Apache-2.0 OR MIT", allow));
  assert.ok(isAllowed("(GPL-2.0-only OR MIT)", allow), "one acceptable choice is enough");
  assert.ok(isAllowed("MIT AND BSD-3-Clause", allow));
  assert.ok(!isAllowed("MIT AND GPL-2.0-only", allow), "AND needs every part");
  assert.ok(isAllowed("(MIT AND BSD-3-Clause) OR GPL-2.0-only", allow));
  assert.ok(isAllowed("Apache-2.0 WITH LLVM-exception", allow));
  assert.ok(!isAllowed("GPL-2.0-only WITH Classpath-exception-2.0", allow));
  assert.ok(!isAllowed("Unknown", allow));
  assert.ok(!isAllowed("", allow));
});

test("lists every package whose licence isn't allowed", () => {
  const report = {
    MIT: [{ name: "a", versions: ["1.0.0"] }],
    "GPL-2.0-only": [{ name: "b", versions: ["2.0.0"] }],
    Unknown: [{ name: "c", versions: ["3.0.0", "3.1.0"] }],
  };
  assert.deepEqual(violations(report, allow), ["b@2.0.0: GPL-2.0-only", "c@3.0.0, 3.1.0: Unknown"]);
});
