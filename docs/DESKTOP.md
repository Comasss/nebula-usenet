# Nebula on Linux and macOS

The desktop app is the same on Windows, Linux and macOS: the same window, tray icon, settings and post-processing. par2cmdline-turbo and 7-Zip come inside every package, so nothing else needs installing.

| File | For | Updates |
|---|---|---|
| `Nebula_<version>_linux_x86_64.AppImage` | Any x64 Linux with a desktop (Ubuntu 22.04, Debian 12, Fedora 36 or later) | By itself, from **Settings → Updates** |
| `Nebula_<version>_linux_amd64.deb` | Debian, Ubuntu, Linux Mint, Pop!_OS… | Nebula tells you and opens the Release; install the new `.deb` |
| `Nebula_<version>_macos_universal.dmg` | macOS 11 or later, Apple Silicon and Intel | Nebula tells you and opens the Release; replace the app |

For a server or NAS without a desktop, use [nebula-server](SERVER.md) or [Docker](DOCKER.md) instead.

## Installing

**AppImage**: download it, make it executable and open it:

```bash
chmod +x Nebula_*_linux_x86_64.AppImage
./Nebula_*_linux_x86_64.AppImage
```

Keep it in a fixed folder (for example `~/Applications`): the `.nzb` association and "Start with the system" point to where it is, and are rewritten on every start if you move it. On distributions without FUSE 2 (Ubuntu 24.04 and later), install `libfuse2t64` or run it with `--appimage-extract-and-run`.

**.deb**:

```bash
sudo apt install ./Nebula_*_linux_amd64.deb
```

It installs `nebula` with `nebula-par2` and `nebula-7zz` in `/usr/bin`, a menu entry and the `.nzb` association. `sudo apt remove nebula` uninstalls it (your data stays).

**macOS**: open the `.dmg` and drag Nebula to Applications. The app isn't signed with an Apple certificate, so the first time macOS refuses to open it: right-click Nebula in Applications → **Open** → **Open**. On macOS 15 and later, if there's no Open button, go to **System Settings → Privacy & Security** and press **Open Anyway** next to Nebula. You only need to do this once.

## What works like on Windows

| | Linux | macOS |
|---|---|---|
| Tray icon and menu | Yes (needs a tray: GNOME needs the *AppIndicator* extension, which Ubuntu includes) | Menu bar |
| Notifications | Yes | Yes (allow them the first time) |
| Open `.nzb` with Nebula | **Settings → System integration**. Also declared by the `.deb` | Declared by the app: Finder → Get Info → Open with → Nebula → Change All |
| Start with the system | `~/.config/autostart/nebula.desktop` | `~/Library/LaunchAgents/com.nebula.downloader.plist` |
| Shut down, sleep, hibernate on finish | `systemctl poweroff / suspend / hibernate` (hibernate needs swap set up for it) | Shut down asks System Events (macOS asks for permission once); sleep and hibernate both sleep |
| Watch the clipboard | Yes (X11, and Wayland through XWayland) | Yes |
| Passwords | The encryption key goes to the keyring (GNOME Keyring, KWallet) through the Secret Service | The key goes to the Keychain |

## Where your data is

| | Linux | macOS |
|---|---|---|
| Settings, queue, history, logs | `~/.config/Nebula` | `~/Library/Application Support/Nebula` |
| Downloads (default) | `~/Downloads/Nebula` | `~/Downloads/Nebula` |

As on Windows, `NEBULA_DATA_DIR` uses another folder (and keeps the key in a file there instead of the keyring, so that folder can be copied to another machine).

### Passwords

Server passwords, API keys and tokens are encrypted with a key. On Windows that key is protected with DPAPI; on Linux and macOS Nebula keeps it in the system keyring, so the settings file alone doesn't reveal them. The first time it runs, Nebula creates the key, saves it in the keyring and checks that it reads back the same before removing the copy in the data folder. If there's no keyring (a minimal desktop without GNOME Keyring or KWallet), the key stays in `secret.key` in the data folder, readable only by your user, as in nebula-server.

## par2 and 7-Zip

The packages include par2cmdline-turbo and 7-Zip (`7zz`, with RAR support) next to the `nebula` executable. If you'd rather use other ones, set their paths in **Settings → Post-processing**; the rows show which one is being used. Without the bundled ones (building from source), Nebula looks for `par2` and `7zz`/`7z` in the `PATH`.

## Building

Linux (Ubuntu 22.04 or later):

```bash
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev \
  libxdo-dev libssl-dev patchelf build-essential automake autoconf
npm install
dev/desktop-tools.sh                     # par2 and 7-Zip into src-tauri/bin, only once
npx tauri build --config src-tauri/tauri.tools.conf.json
```

The AppImage and the `.deb` end up in `target/release/bundle/`. Without `--config src-tauri/tauri.tools.conf.json` the packages are built without par2 and 7-Zip (and `dev/desktop-tools.sh` isn't needed), which is enough for `npx tauri dev`.

macOS (Xcode command line tools, Homebrew):

```bash
brew install automake autoconf
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm install
dev/desktop-tools.sh universal-apple-darwin
npx tauri build --target universal-apple-darwin --config src-tauri/tauri.tools.conf.json
```

The `.dmg` ends up in `target/universal-apple-darwin/release/bundle/dmg/`. GitHub Actions builds both (`linux` and `macos` jobs in `.github/workflows/build.yml`) and publishes them in the Release.
