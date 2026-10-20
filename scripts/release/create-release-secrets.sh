#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Creates Kipple's two long-lived release secrets and stores them as GitHub Actions secrets:
#   - the updater signing key (minisign, via `tauri signer`), and
#   - the self-signed macOS code-signing certificate "Kipple Release Signing" (ADR-0008).
# Losing either is costly (updates stop verifying / macOS permissions reset), so back up the
# output folder offline (password manager or an encrypted USB stick), then delete the local copy.
#
#   bash scripts/release/create-release-secrets.sh [--no-upload] [OUTPUT_DIR]
#
# OUTPUT_DIR defaults to ~/kipple-release-secrets and must not exist yet: this never overwrites
# keys. Prints no private key and no password, so its output is safe to share or paste.
# The public updater key goes into app/src-tauri/tauri.release.conf.json (commit that); set
# KIPPLE_RELEASE_CONF to write it elsewhere (the tests do).
# macOS only (the certificate is made with the system LibreSSL so `security import` accepts it).
set -euo pipefail

UPLOAD=1
if [ "${1:-}" = "--no-upload" ]; then UPLOAD=0; shift; fi
DIR="${1:-$HOME/kipple-release-secrets}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RELEASE_CONF="${KIPPLE_RELEASE_CONF:-$ROOT/app/src-tauri/tauri.release.conf.json}"
CN="Kipple Release Signing"
OPENSSL=/usr/bin/openssl

die() { echo "error: $*" >&2; exit 1; }
[ "$(uname)" = Darwin ] || die "run this on macOS"
[ ! -e "$DIR" ] || die "$DIR already exists; refusing to overwrite existing keys"
[ -f "$RELEASE_CONF" ] || die "$RELEASE_CONF not found"
if [ "$UPLOAD" = 1 ]; then
  command -v gh >/dev/null || die "the GitHub CLI (gh) is needed to upload the secrets"
  gh auth status >/dev/null 2>&1 || die "run 'gh auth login' first"
  REPO="$(cd "$ROOT" && gh repo view --json nameWithOwner -q .nameWithOwner)" \
    || die "can't tell which GitHub repo this checkout belongs to"
fi

umask 077
mkdir -p "$DIR"
"$OPENSSL" rand -base64 32 | tr -d '\n' > "$DIR/updater.password"
"$OPENSSL" rand -base64 32 | tr -d '\n' > "$DIR/certificate.password"

# 1. Updater key: updater.key (private, password-protected) + updater.key.pub. `tauri signer`
# only takes the password as an argument, so it's visible in `ps` for a second: run this on your
# own (single-user) Mac.
(cd "$ROOT/app" && pnpm -s tauri signer generate --ci -p "$(cat "$DIR/updater.password")" \
  -w "$DIR/updater.key") > "$DIR/tauri-signer.log" 2>&1 \
  || die "tauri signer failed, see $DIR/tauri-signer.log"
[ -s "$DIR/updater.key" ] && [ -s "$DIR/updater.key.pub" ] || die "updater key files missing"

# 2. Self-signed code-signing certificate, valid 20 years (ADR-0008: never rotated)
cat > "$DIR/certificate.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $CN
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CNF
"$OPENSSL" req -x509 -newkey rsa:3072 -nodes -days 7300 -keyout "$DIR/certificate.key.pem" \
  -out "$DIR/certificate.pem" -config "$DIR/certificate.cnf" 2>/dev/null
"$OPENSSL" pkcs12 -export -inkey "$DIR/certificate.key.pem" -in "$DIR/certificate.pem" -name "$CN" \
  -out "$DIR/certificate.p12" -passout "file:$DIR/certificate.password"
"$OPENSSL" pkcs12 -in "$DIR/certificate.p12" -noout -passin "file:$DIR/certificate.password" \
  || die "the certificate bundle doesn't open with its password"
rm "$DIR/certificate.key.pem" # the private key lives on only inside the password-protected .p12

cat > "$DIR/README.txt" <<TXT
Kipple release secrets, created $(date -u +%Y-%m-%d). BACK THIS FOLDER UP OFFLINE, then delete it.

updater.key / updater.password   Signs updates. GitHub secrets TAURI_SIGNING_PRIVATE_KEY(_PASSWORD).
updater.key.pub                  Public; also in app/src-tauri/tauri.release.conf.json.
certificate.p12 / .password      "$CN", signs macOS builds (ADR-0008). GitHub secrets
                                 APPLE_CERTIFICATE (base64 of the .p12) and APPLE_CERTIFICATE_PASSWORD.
certificate.pem                  The public certificate.

Never rotate these unless they leak: a new updater key breaks updates for every installed copy,
and a new certificate makes macOS ask every user for folder permissions again.
TXT

# The public key goes into the release config (plugins.updater.pubkey), where the bundler needs it.
KEY_PUB="$DIR/updater.key.pub" node -e '
  const fs = require("node:fs");
  const file = process.argv[1];
  const conf = JSON.parse(fs.readFileSync(file, "utf8"));
  conf.plugins = { ...conf.plugins, updater: { ...conf.plugins?.updater, pubkey: fs.readFileSync(process.env.KEY_PUB, "utf8").trim() } };
  fs.writeFileSync(file, JSON.stringify(conf, null, 2) + "\n");
' "$RELEASE_CONF"
echo "Created in $DIR:"
ls -1 "$DIR" | sed 's/^/  /'
echo "Updater public key (not secret) written to ${RELEASE_CONF#"$ROOT"/}: commit it."
echo "Certificate SHA-256: $("$OPENSSL" x509 -in "$DIR/certificate.pem" -noout -fingerprint -sha256 | cut -d= -f2)"

if [ "$UPLOAD" = 1 ]; then
  gh secret set TAURI_SIGNING_PRIVATE_KEY --repo "$REPO" < "$DIR/updater.key"
  gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo "$REPO" < "$DIR/updater.password"
  base64 -i "$DIR/certificate.p12" | gh secret set APPLE_CERTIFICATE --repo "$REPO"
  gh secret set APPLE_CERTIFICATE_PASSWORD --repo "$REPO" < "$DIR/certificate.password"
  gh secret set APPLE_SIGNING_IDENTITY --repo "$REPO" --body "$CN"
  echo "Uploaded 5 GitHub Actions secrets to $REPO:"
  gh secret list --repo "$REPO" | sed 's/^/  /'
fi
echo "Next: back up $DIR offline (it is the only copy of the private keys), then delete it."
