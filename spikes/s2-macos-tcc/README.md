# Spike S2 (K-0.3): macOS folder permissions across updates

**Question:** Kipple ships without Apple notarisation. Does macOS keep Kipple's folder permissions (Desktop, Documents, Downloads) after an update, or does every update ask again, or even lose access silently?

**Why it matters:** if every update resets permissions, users get re-prompted on every release, and scheduled rules could fail silently. The answer decides how the macOS release is signed ([ARCHITECTURE.md §11, S2](../../docs/ARCHITECTURE.md#12-risks--spikes-do-these-first)).

**Hypothesis:** macOS remembers a permission for the app's code-signing *designated requirement* (DR).
- **Ad-hoc** signing (Tauri's default without a certificate) gives a DR based on the binary's hash. **It changes with every build**, so permissions are expected to reset on every update.
- A **self-signed certificate** gives a DR based on the bundle ID and the certificate. **It stays the same across builds**, so permissions are expected to survive updates. It's free.

## What's here

| File | Purpose |
|---|---|
| `src-tauri/`, `ui/` | Minimal Tauri 2.12.1 app: shows its version and has **Read Desktop / Documents / Downloads** buttons |
| `make-cert.sh` | Creates a self-signed code-signing certificate in a **separate keychain file** under `out/signing/`. Your login keychain isn't touched |
| `build.sh` | Builds 4 apps (ad-hoc and self-signed, each v0.0.1 and v0.0.2) as quarantined `.dmg` "downloads" plus updater-style copies |
| `inspect.sh` | Compares the DR of v0.0.1 vs v0.0.2 per mode (automatic evidence) |
| `simulate-update.sh` | Swaps the installed app for v0.0.2 the way an in-app updater does |

```sh
./build.sh        # ~5 min the first time
./inspect.sh      # writes out/inspect.txt
```

## Manual test checklist (~15 minutes)

Do this for **ad-hoc first, then self-signed**. Your own user account is fine, because these are new app IDs. To start over, reset the permissions with `tccutil reset All app.kipple.spike.tcc.adhoc` (or `.selfsigned`).

**A. First install (simulated download)**
1. Double-click `out/dmg/<mode>-0.0.1.dmg` and drag the app to **/Applications** or **~/Applications**.
2. Open it from /Applications. Write down **exactly what macOS shows** (dialog title and buttons).
3. If it's blocked: open **System Settings → Privacy & Security**, scroll down, and click **Open Anyway**. Write down the steps it took.
4. In the app (it shows **v0.0.1**), click **Read Desktop**. macOS should ask *"… would like to access files in your Desktop folder"*. Click **Allow**. Expected result: ✅ N entries.
5. Click **Read Desktop** again. It should show ✅ without a prompt.
6. Quit the app (⌘Q).

**B. Update (like the in-app updater)**

7. In Terminal, in this folder, run `./simulate-update.sh <mode>`.
8. Open the app from /Applications. It should show **v0.0.2**. Did Gatekeeper complain again? (Write it down.)
9. Click **Read Desktop**. Did macOS **ask again**, show ✅ **without asking**, or show ❌ **"Operation not permitted"** without asking?
10. Quit the app.

**C. Optional: update by downloading a new .dmg**

11. Open `out/dmg/<mode>-0.0.2.dmg` and drag it over the installed app (choose Replace). Then repeat steps 8–9.

## Results

Machine: macOS 27.0.1 (Apple Silicon) · Tester: project maintainer · Date: 2026-10-20 · Installed into `~/Applications` (works the same as `/Applications`)

| Step | Ad-hoc | Self-signed |
|---|---|---|
| A2–A3. First launch | Blocked; **Open Anyway** in System Settings → Privacy & Security needed | Same |
| A4. Desktop prompt shown & allowed? | Yes | Yes |
| **B9. Desktop access after update** | ❌ **macOS asked again** | ✅ **Remembered, no prompt** |
| C. Update via new .dmg | Not tested (optional) | Not tested (optional) |

**Conclusion:** the hypothesis holds. Ship macOS builds signed with a stable self-signed certificate ([ADR-0008](../../docs/adr/0008-macos-signing-without-notarisation.md), Accepted).

Cleanup: drag both "Kipple TCC Spike" apps from Applications to the Trash, and run `tccutil reset All app.kipple.spike.tcc.adhoc` and `tccutil reset All app.kipple.spike.tcc.selfsigned`.

Automatic evidence (`out/inspect.txt`): see the **"Designated requirement"** section in [ADR-0008](../../docs/adr/0008-macos-signing-without-notarisation.md).
