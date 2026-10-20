#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Creates a self-signed code-signing certificate in a DEDICATED keychain file under out/signing/.
# Your login keychain and keychain search list are NOT touched. Throwaway: delete out/ to remove.
set -euo pipefail
cd "$(dirname "$0")"

OUT="out/signing"
KC="$PWD/$OUT/kipple-spike.keychain-db"
PW="kipple-spike-throwaway"   # protects a throwaway keychain only
CN="Kipple Spike Self-Signed"
OPENSSL=/usr/bin/openssl      # system LibreSSL: produces PKCS#12 files that `security import` accepts

mkdir -p "$OUT"
if [ -f "$KC" ]; then echo "Keychain already exists: $KC"; else
  cat > "$OUT/cert.cnf" <<EOF
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
EOF
  "$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -keyout "$OUT/key.pem" -out "$OUT/cert.pem" -config "$OUT/cert.cnf"
  "$OPENSSL" pkcs12 -export -inkey "$OUT/key.pem" -in "$OUT/cert.pem" \
    -name "$CN" -out "$OUT/cert.p12" -passout "pass:$PW"

  security create-keychain -p "$PW" "$KC"
  security set-keychain-settings "$KC"            # no auto-lock
  security unlock-keychain -p "$PW" "$KC"
  security import "$OUT/cert.p12" -k "$KC" -P "$PW" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$PW" "$KC" >/dev/null
fi

security unlock-keychain -p "$PW" "$KC"
# Without -v: also lists identities whose certificate isn't trusted (self-signed).
HASH=$(security find-identity -p codesigning "$KC" | awk '/\)/ {print $2; exit}')
[ -n "$HASH" ] || { echo "No code-signing identity found in $KC" >&2; exit 1; }
echo "$HASH" > "$OUT/identity.txt"
echo "Self-signed identity: $HASH ($CN) in $KC"
