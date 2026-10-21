# Installing Kipple

*Nederlands: [INSTALL.nl.md](INSTALL.nl.md)*

Kipple is free and has no paid code-signing certificate, so Windows and macOS warn you the first time you open it. That warning means "this app isn't from a paid-up developer account", not "this app is harmful". This page shows how to get past it once, and how to check that the file you downloaded is the one we built.

Download Kipple from the [Releases page](https://github.com/aaronnoels2010/kipple/releases/latest). Pick the file for your system:

| System | File | Notes |
|---|---|---|
| **Windows 10/11** | `Kipple_<version>_x64-setup.exe` | Recommended. Installs for your account only, no admin needed, updates itself |
| | `Kipple_<version>_x64_en-US.msi` | For administrators installing for every user |
| | `Kipple_<version>_x64-portable.zip` | No installation: unzip and run `Kipple.exe`. Keeps everything in a `KippleData` folder next to it, so it runs from a USB stick. Doesn't update itself |
| **macOS 11 or later** (Apple silicon and Intel) | `Kipple_<version>_universal.dmg` | |
| **Linux** | `Kipple_<version>_amd64.AppImage` | Runs on most distributions, updates itself |
| | `Kipple_<version>_amd64.deb` | Debian, Ubuntu, Linux Mint… |
| | `Kipple-<version>-1.x86_64.rpm` | Fedora, openSUSE… |

## Windows

1. Run the `-setup.exe`.
2. Windows SmartScreen says **"Windows protected your PC"**. Click **More info**, check that it says *Kipple*, then click **Run anyway**.
3. Follow the installer. Kipple appears in the Start menu.

You only see this warning for a new version you haven't run before. The portable `.exe` shows it the first time too.

**Portable version:** extract the whole zip first (*Extract all…*), then run `Kipple.exe` from the extracted `Kipple` folder. Run from inside the zip, it can't see its `portable` file and uses your user profile like the installed version. If the installed Kipple is already running, starting the portable one just brings the installed one to the front: quit that first.

## macOS

1. Open the `.dmg` and drag **Kipple** into **Applications**.
2. Open Kipple from Applications. macOS says it **can't verify** that Kipple is free of malware. Click **Done** (not *Move to Bin*).
3. Open **System Settings → Privacy & Security**, scroll down to *"Kipple" was blocked…* and click **Open Anyway**. Confirm with your password or Touch ID.
4. Kipple opens. From now on it opens normally.

After an update, macOS may ask once more (step 3). Folder permissions you gave Kipple are kept: every release is signed with the same certificate, *Kipple Release Signing*. To check that your copy carries it:

```sh
codesign -dvv /Applications/Kipple.app 2>&1 | grep Authority
# Authority=Kipple Release Signing
```

## Linux

**AppImage**

```sh
chmod +x Kipple_*_amd64.AppImage
./Kipple_*_amd64.AppImage
```

If it doesn't start and mentions FUSE, install it: `sudo apt install libfuse2t64` (Ubuntu 24.04 and later) or `sudo apt install libfuse2` (older Ubuntu and Debian).

**.deb**: `sudo apt install ./Kipple_*_amd64.deb`

**.rpm**: `sudo dnf install ./Kipple-*.x86_64.rpm`

## Check your download

Every release has a `SHA256SUMS.txt` with the checksum of each file. Download it into the same folder as your file, then:

**macOS / Linux**

```sh
shasum -a 256 --check --ignore-missing SHA256SUMS.txt
# Kipple_…: OK
```

**Windows (PowerShell)**

```powershell
(Get-FileHash .\Kipple_<version>_x64-setup.exe -Algorithm SHA256).Hash.ToLower()
```

Compare the result with the line for that file in `SHA256SUMS.txt`: they must match exactly.

**Proof it was built from this repository.** Each file also has a GitHub build attestation, which proves that GitHub Actions built it from the Kipple source code. With the [GitHub CLI](https://cli.github.com):

```sh
gh attestation verify Kipple_<version>_universal.dmg --repo aaronnoels2010/kipple
```

## Removing Kipple

- **Windows:** *Settings → Apps → Installed apps → Kipple → Uninstall*.
- **macOS:** drag Kipple from Applications to the Bin.
- **Linux:** delete the AppImage, or `sudo apt remove kipple` / `sudo dnf remove kipple`.

Kipple's settings and index stay in your user profile after that (the portable version keeps them in its `KippleData` folder: delete that folder too). **Settings → Remove all Kipple data** will remove them from inside the app (coming in a later version, before 1.0).
