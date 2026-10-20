# Security Policy

Kipple moves, renames and deletes files and previews untrusted downloads, so security bugs can mean **lost data**. We take reports seriously.

## Supported versions

| Version | Supported |
|---|---|
| Latest release | ✅ |
| Older releases | ❌ (please update first) |
| Pre-releases / `main` | ✅ best effort |

Kipple has no releases yet. This table applies from v0.1.0 onwards.

## Reporting a vulnerability

**Please don't open a public issue.** Use GitHub's private vulnerability reporting instead:

👉 **[Report a vulnerability](https://github.com/aaronnoels2010/kipple/security/advisories/new)**

Please include:
- what an attacker (or a malicious file) can do, and under which conditions,
- steps to reproduce, or a proof of concept,
- the Kipple version and OS,
- a **diagnostics export** if relevant. It is redacted by default, so only opt in to file paths if they are needed and safe to share.

This is a volunteer project. We aim to acknowledge reports **within 7 days** and to agree on a disclosure timeline with you. Credit is given in the release notes unless you prefer otherwise. There is no bug bounty.

## What we especially want to hear about

- Ways Kipple could **delete, overwrite or lose files** without the user's explicit consent: journal or recovery bugs, the Trash being bypassed, protected paths being touched.
- **Escaping the preview sandbox:** script execution from a previewed file (SVG, PDF, Markdown, HTML), navigation away from the app origin, or access to files outside the item being previewed.
- **Path traversal** or acting on paths that didn't come from the index or a native dialog.
- **Cloud placeholder hydration:** online-only files being downloaded without consent.
- **Updater issues:** installing an update that isn't signed with the project's update key.
- **Privacy leaks:** file paths or names leaving the machine, or escaping log redaction.

## Out of scope

- Missing code signing or notarisation warnings. Releases are unsigned by design; see the install guide.
- Issues that need an already-compromised user account or OS.
- Third-party dependencies, unless Kipple uses them in an unsafe way. Report those upstream as well.
