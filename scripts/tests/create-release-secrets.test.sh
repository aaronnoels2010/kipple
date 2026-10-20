#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Tests for scripts/release/create-release-secrets.sh (macOS only; skipped elsewhere). Creates
# throwaway keys in a temp dir with --no-upload. Run: bash scripts/tests/create-release-secrets.test.sh
set -u
[ "$(uname)" = Darwin ] || { echo "skip - macOS only"; exit 0; }

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/../release/create-release-secrets.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
# A copy of the release config, so the real one in the repo is never touched.
export KIPPLE_RELEASE_CONF="$TMP/tauri.release.conf.json"
cp "$HERE/../../app/src-tauri/tauri.release.conf.json" "$KIPPLE_RELEASE_CONF"

pass=0
fail=0
check() { # check <description> <command...>
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then pass=$((pass + 1)); echo "ok   - $desc"; else fail=$((fail + 1)); echo "FAIL - $desc"; fi
}

out="$(bash "$SCRIPT" --no-upload "$TMP/secrets" 2>&1)"
check "succeeds" test $? -eq 0
for f in updater.key updater.key.pub updater.password certificate.p12 certificate.password certificate.pem README.txt; do
  check "creates $f" test -s "$TMP/secrets/$f"
done
check "removes the bare certificate key" test ! -e "$TMP/secrets/certificate.key.pem"
check "private files are owner-only" test "$(stat -f %Lp "$TMP/secrets/updater.key")" = 600
check "the .p12 opens with its password" /usr/bin/openssl pkcs12 -in "$TMP/secrets/certificate.p12" -noout -passin "file:$TMP/secrets/certificate.password"
check "the certificate is for code signing" sh -c "/usr/bin/openssl x509 -in '$TMP/secrets/certificate.pem' -noout -text | grep -q 'Code Signing'"
check "writes the public key into the release config" node -e '
  const fs = require("node:fs");
  const conf = JSON.parse(fs.readFileSync(process.env.KIPPLE_RELEASE_CONF, "utf8"));
  const key = fs.readFileSync(process.argv[1], "utf8").trim();
  process.exit(conf.plugins.updater.pubkey === key && conf.bundle.createUpdaterArtifacts ? 0 : 1);
' "$TMP/secrets/updater.key.pub"
leaks=0
for f in updater.password certificate.password updater.key; do
  case "$out" in *"$(cat "$TMP/secrets/$f")"*) leaks=1 ;; esac
done
check "prints no password or private key" test "$leaks" -eq 0
second="$(bash "$SCRIPT" --no-upload "$TMP/secrets" 2>&1)"
check "refuses to overwrite existing keys" test $? -ne 0
check "  ...and says why" sh -c "printf '%s' \"$second\" | grep -q 'refusing to overwrite'"

echo "# $pass passed, $fail failed"
[ "$fail" -eq 0 ]
