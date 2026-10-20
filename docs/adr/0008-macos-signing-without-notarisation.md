# ADR-0008: Sign macOS builds with a stable self-signed certificate, not ad-hoc

- Status: **Proposed**. The automatic evidence is in; the manual TCC test is pending (see [spike S2](../../spikes/s2-macos-tcc/README.md))
- Date: 2026-10-20
- Task: K-0.3 (spike S2)

## Context
Kipple has no budget for an Apple Developer ID, so macOS builds aren't notarised. macOS still needs *some* signature: Apple Silicon refuses to run unsigned code, so Tauri ad-hoc signs by default.

Kipple needs folder permissions (TCC: Desktop, Documents, Downloads, maybe Full Disk Access). **macOS stores each grant against the app's code-signing *designated requirement* (DR).** If the DR changes on update, macOS treats the new version as a different app. The user is asked again, or a background rule fails with "Operation not permitted".

## Evidence (spike S2, `inspect.sh`, macOS 27.0.1, Apple Silicon, Tauri 2.12.1)

| Signing | DR v0.0.1 | DR v0.0.2 | Stable? |
|---|---|---|---|
| Ad-hoc (`signingIdentity: "-"`) | `cdhash H"d970f887…"` | `cdhash H"7e209876…"` | ❌ changes with every build |
| Self-signed certificate | `identifier "app.kipple.spike.tcc.selfsigned" and certificate leaf = H"5969d813…"` | identical | ✅ stable |

- `codesign` accepts the self-signed certificate **without it being trusted** in any keychain. A dedicated keychain file is enough, so there are no admin prompts and the login keychain isn't touched.
- Gatekeeper (`spctl`) rejects **both** variants, as expected without notarisation. Either way, users approve the app once via *System Settings → Privacy & Security → Open Anyway*.

**Manual TCC confirmation:** *pending*. The results table is in `spikes/s2-macos-tcc/README.md`.

## Decision (proposed)
1. Sign every macOS release with **one long-lived self-signed code-signing certificate** ("Kipple Release Signing"), using a fixed bundle identifier (`app.kipple.desktop`).
2. Never ship ad-hoc signed releases. Ad-hoc is fine for local development builds only.
3. Treat the certificate's **private key as a release secret**, like the updater key:
   - It is stored only as a GitHub Actions secret, with an offline backup.
   - It is valid for 10+ years and **never rotated** unless compromised. Rotating it changes the DR, so every user would have to grant permissions once more.

## Consequences
- ✅ Folder permissions should survive updates (to be confirmed manually). Rules and scheduled scans keep working after an update.
- ✅ Free, and it works in CI with a temporary keychain.
- ⚠️ **Security:** anyone holding the private key can sign an app with Kipple's identifier and DR, and that app would **inherit Kipple's folder permissions without a prompt**. Mitigations:
  - The key is only ever in CI secrets and offline backup.
  - Release builds are only produced by the public CI workflow.
  - A key compromise is handled like an incident: new certificate, a release note explaining the one-time re-grant, and a SECURITY advisory.
- ⚠️ This doesn't help with Gatekeeper. First launch still needs "Open Anyway" (covered by the install guide, K-1.10).
- **Open for K-1.9:** check whether Tauri's bundler can sign with an *untrusted* identity from a CI keychain (`APPLE_CERTIFICATE` + `signingIdentity`), so the updater `.app.tar.gz` carries the right signature. If it can't, use a post-bundle re-sign step that also regenerates the updater artifact.
