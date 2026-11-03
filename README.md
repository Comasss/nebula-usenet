<div align="center">

<img src="src/assets/logo.svg" width="96" alt="Nebula Usenet logo">

# Nebula Usenet

**A modern Usenet (NZB) client for Windows and Android.**<br>
Search all your indexers, download over NNTP or debrid, repair and extract automatically, all from one fast, dark, animated app. An alternative to SABnzbd and NZBGet.

[![Latest release](https://img.shields.io/github/v/release/Comasss/nebula?label=release&color=7c5cff)](https://github.com/Comasss/nebula/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Comasss/nebula/total?color=22c3e6)](https://github.com/Comasss/nebula/releases)
[![Build](https://img.shields.io/github/actions/workflow/status/Comasss/nebula/build.yml?branch=main&label=build)](https://github.com/Comasss/nebula/actions/workflows/build.yml)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Android-555)
[![License: PolyForm Noncommercial](https://img.shields.io/badge/license-PolyForm%20Noncommercial-orange)](LICENSE.md)

[**Download**](https://github.com/Comasss/nebula/releases/latest) · [Features](#features) · [Screenshots](#screenshots) · [Newznab API](docs/API.md) · [Building](#building)

<img src="https://github.com/Comasss/nebula/raw/screenshots/downloads.png" alt="Nebula Usenet: Downloads view with live speed graph, queue and post-processing" width="900">

</div>

## Features

- **Rust engine** (Tauri 2 + tokio): NNTP with TLS, multiple connections, backup servers by priority, yEnc decoding with CRC checks, direct-to-disk writes and resuming after the app is closed.
- **Built-in post-processing**: PAR2 verification and repair (par2cmdline-turbo) that downloads only the recovery blocks it needs, and RAR/7z/ZIP extraction (7-Zip), with a progress bar, percentage and speed. Both tools ship **inside the .exe**.
- **Newznab indexer search** (NZBGeek, NZBFinder, DrunkenSlug…): several at once or only the ones you pick, with category filter, sorting and optional grouping of duplicates (the same release reposted to several indexers shows up once and, if one indexer fails to download, the next one is used). Download with one click or save the `.nzb` to your PC.
- **Downloads through debrid services** (TorBox Pro and Premiumize): Nebula sends them the NZB, they download it (and repair/extract it) on their servers, and Nebula fetches the loose files over HTTPS with several ranged connections, to the same place and in the same layout as over NNTP (never a `.zip`). You choose the default method and, for each NZB, NNTP or the service you want.
- **Statistics**: overall NZB success rate and one per NNTP server (plus debrid services), for the last 7, 30 or 90 days, the last year, all time or a custom date range. Each server also shows how many of the articles it was asked for it actually had. An **Indexers** tab shows each indexer's average response time, average results per search (and for all of them combined), error rate, NZBs grabbed and how many of those downloaded successfully.
- **Newznab API compatible with NZBHydra2**: optional, with its own API key. Sonarr, Radarr, Prowlarr and other apps can search all your indexers at once through Nebula as if it were a single indexer. See [docs/API.md](docs/API.md).
- **Move your settings between devices with a QR code**: in **Settings → Move to another device** the PC shows a QR code that you scan with Nebula on your phone. To go the other way, the PC shows a QR code with its address on your Wi-Fi and a one-time key; the phone scans it and sends its settings encrypted over the local network. Servers, indexers, debrid services, the API and preferences travel; folders, history and system-specific options (tray, start with Windows, vibration…) stay as they are. If you can't scan, copy the code and paste it on the other device.
- **Pick your features** on first launch (at least one): indexers only (save `.nzb` files), indexers + NNTP, or NNTP only.
- **12 languages**: English, Spanish, Simplified Chinese, Hindi, Portuguese (Brazil), French, German, Russian, Japanese, Arabic, Italian and Catalan. On startup it uses the Windows language (or English if it isn't available), and you can change it in **Settings → Language**; the tray menu, notifications and error messages are translated too.
- **Svelte 5 interface**: first-run NNTP credentials wizard, Downloads / History / Statistics / Servers / Settings / Log tabs, `.nzb` drag and drop, link pasting, configurable clipboard watching, tray icon and notifications.

## Screenshots

| Search every indexer at once | Statistics per server and indexer | History, retries and repairs |
|:---:|:---:|:---:|
| <img src="https://github.com/Comasss/nebula/raw/screenshots/search.png" alt="Search across several Newznab indexers with merged duplicates"> | <img src="https://github.com/Comasss/nebula/raw/screenshots/stats.png" alt="Success rate statistics per NNTP server and debrid service"> | <img src="https://github.com/Comasss/nebula/raw/screenshots/history.png" alt="History with completed and failed downloads and retry actions"> |

**Android:** the same engine and features, with a phone layout and a floating tab bar.

<p align="center"><img src="https://github.com/Comasss/nebula/raw/screenshots/android.png" alt="Nebula Usenet on Android: downloads, search and statistics" width="760"></p>

<sub>Screenshots use demo data from the built-in preview (<code>dev/preview.html</code>).</sub>

## Download

[Releases](../../releases) has two options for Windows 10/11 x64:

- `Nebula_<version>_x64-setup.exe`: installer (shortcut, uninstaller and `.nzb` file association).
- `Nebula_<version>_x64_portable.exe`: a single executable, no installation.

Both need WebView2, which comes with Windows 11 (the installer downloads it if missing).

For Android 7+ on 64-bit ARM phones (practically all of them) there is `Nebula_<version>_android_arm64.apk`. Download it on the phone, allow installing from that source and open it.

## Android

The Android app is the same program: the Rust engine and the Svelte interface run through Tauri's Android support, so it has the same features (NNTP, PAR2 repair and extraction, indexers, debrid, statistics, 12 languages) with a layout designed for phones:

- A floating "liquid glass" tab bar (Search, Downloads, History, Stats, Settings). Slide your finger along it to switch tabs, and it shrinks while you scroll.
- Settings is one place with sections: indexers, servers, debrid and the log live there next to the preferences.
- Press and hold **Download** on a search result to choose how to get it: all your NNTP servers, one specific server, or one of your debrid services.
- Press and hold a download or a history entry for its actions (pause, move, retry, remove...).
- Ripples, a hold ring, haptic feedback (can be turned off in Settings › Look), swipe-to-dismiss notifications and a floating add button.
- The launcher icon is the same as on Windows (`dev/android-icons.mjs` regenerates it from `app-icon.svg`).

- Downloads go to `Download/` by default. To save to any other folder, grant **access to all files** when Nebula asks (or in **Settings → App and notifications**).
- While something is downloading, Nebula keeps a notification visible so Android doesn't stop it in the background.
- Add NZBs with the **Add NZB** button, by opening a `.nzb` from a file manager or browser, by sharing a file or link to Nebula, or by copying a link (Nebula offers it when you come back to the app if clipboard watching is enabled).
- PAR2 and 7-Zip are compiled for Android and ship inside the APK (`lib7zz.so`, `libpar2.so`).
- The APK is signed with a key kept only in the repository secrets `ANDROID_KEYSTORE_BASE64` (the `.jks` in base64), `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD`. Builds on `main` and `v*` tags fail without them. Without the secrets (pull requests from forks, local builds) the APK is signed with the Android debug key: it works for testing but won't install over the official app.

## Usage

- Add an NZB: the **Add NZB** button, drag it onto the window, double-click a `.nzb` (if the association is enabled) or paste a link into the top bar. Nebula downloads the NZB, parses it and asks where to save it.
- Archive passwords: read from the NZB (`<meta type="password">`) or from the name (`Name {{password}}`). If they're missing, History offers **Retry extraction**.
- Debrid downloads: in the **Debrid** tab add your TorBox API key (NZB downloads need the Pro plan) or your Premiumize one (premium account) and press **Test**. In **Settings → NNTP / Debrid** you choose the default method:
  - **Usenet (NNTP)**: your servers. When adding an NZB you can still pick a debrid service under "Download with".
  - **Debrid**: the first service in the list is used; if it fails the next one is tried and, if you enable it, NNTP as a last resort. If you have no NNTP servers, debrid is used automatically.

  Each download goes through three stages: sending the NZB, downloading on the service (its progress and speed are shown) and downloading to the PC (8 connections and 32 MB chunks by default, resumable after closing Nebula). Afterwards it is verified/extracted by the usual post-processing if needed and, optionally, removed from the service. When adding an NZB, if TorBox already has it cached it is marked as instant (⚡).
- Translations: UI strings live in `src/locales/<code>.json` and engine strings in `src-tauri/locales/<code>.json` (`es` is the original). To add a language, create both files and add it to `LANGUAGES` in `src/lib/i18n.svelte.js` and to `LOCALES` in `src-tauri/src/i18n.rs`; `cargo test` checks that every translation has the same keys and variables.
- Data: `%APPDATA%\Nebula` (settings, queue, history, statistics, logs). Server passwords are encrypted with DPAPI.
  The `NEBULA_DATA_DIR` environment variable lets you use another folder (portable mode or testing).

## Updates

Nebula checks the latest [Release](../../releases) on GitHub when it opens and every 6 hours (**Settings → Updates** turns this off). When there's a newer version a window shows what's new; you can update right away, close it ("Not now", it comes back on the next launch) or skip that version. **Settings → Updates** always lets you check by hand and install the version found.

- The file is downloaded inside the app and checked against `SHA256SUMS.txt` before installing.
- Windows installer: the new `-setup.exe` runs silently, closes Nebula and reopens it updated. Portable exe: the running `.exe` is swapped for the new one and Nebula restarts.
- Android: the new APK is downloaded and the system installer opens. The first time, Android asks to allow Nebula to install apps.
- Only a higher version number counts as an update, so a rebuild published under the same version isn't offered.

## Automatic builds (GitHub Actions)

`.github/workflows/build.yml` builds Nebula on `windows-latest` (and the Android APK on `ubuntu-latest`) on every push and pull request:

- Runs the engine tests (`cargo test`) and builds the interface.
- Produces the installer and the portable exe and uploads them as a run artifact (**Actions** tab → the run → **Artifacts**).
- On every push to `main` it publishes the **Release** `vX.Y.Z` for the version in `src-tauri/tauri.conf.json` with `Nebula_<version>_x64-setup.exe`, `Nebula_<version>_x64_portable.exe`, `Nebula_<version>_android_arm64.apk` and `SHA256SUMS.txt`:
  - If that version has no Release yet, it creates it.
  - If this same workflow created it, it updates it with the new build (and moves the tag to the latest commit).
  - If you published it by hand, it leaves it alone.

  To publish a new version, bump the number in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json` (for example, `npm version 1.3.0 --no-git-tag-version` for `package.json`). You can also publish by pushing a `vX.Y.Z` tag with that same version.

## Building

Requirements: Rust (MSVC), Node.js and WebView2 (included in Windows 11).

```bash
npm install
npx tauri build
```

This produces `src-tauri/target/release/nebula.exe` (portable) and the installer in `src-tauri/target/release/bundle/nsis/`.

Development with hot reload:

```bash
npx tauri dev
```

Android (needs the Android SDK + NDK, Java 17+ and `rustup target add aarch64-linux-android`):

```bash
export NDK_HOME=$ANDROID_HOME/ndk/<version>
src-tauri/gen/android/build-native-tools.sh   # 7-Zip and par2 for arm64, only once
npx tauri android build --apk --target aarch64
```

## Testing

```bash
cd src-tauri && cargo test
```

`dev/mock_nntp.py` is a mock NNTP server that publishes a folder as yEnc articles, generates the `.nzb` and serves it over HTTP. It can simulate missing articles (`--missing`), a connection limit (`--max-conns`) and speed (`--kbps`):

```bash
python dev/mock_nntp.py --src folder_to_publish --nzb test.nzb --missing 3 --kbps 1500
```

The mock API also exposes a Newznab indexer at `http://127.0.0.1:8119/api` (API key `--indexer-key`, `testkey` by default). Test the indexer client against it or against a real one:

```bash
cd src-tauri && NEBULA_NEWZNAB=http://127.0.0.1:8119 NEBULA_NEWZNAB_KEY=testkey cargo test live_indexer -- --ignored --nocapture
```

The same server mocks the **TorBox** and **Premiumize** APIs (they receive the NZB, "download" for `--debrid-secs` seconds and serve the files from `--src` with ranges). To test debrid downloads without an account, start Nebula pointed at them and use the API key `--debrid-key` (`debridkey` by default); `--debrid-kbps` limits the speed and `--debrid-fail` simulates a failed download:

```powershell
$env:NEBULA_TORBOX_URL = "http://127.0.0.1:8119/torbox/v1/api"
$env:NEBULA_PREMIUMIZE_URL = "http://127.0.0.1:8119/premiumize/api"
$env:NEBULA_DATA_DIR = "$env:TEMP\nebula-test"
npx tauri dev
```

`dev/preview.html` shows the interface in the browser with a mocked Tauri API (`npm run dev` and open `http://localhost:1420/dev/preview.html`; it accepts `?setup`, `?feat`, `?idxonly`, `?add`, `?clipboard`, `?nodebrid`, `?via=debrid` and `?lang=<code>`).

To check the connection to a real server without credentials (TCP + TLS + greeting):

```bash
cd src-tauri && NEBULA_PROBE=news.server.com:563:ssl cargo test probe -- --ignored --nocapture
```

## License

Nebula Usenet is source-available under the [PolyForm Noncommercial License 1.0.0](LICENSE.md). You can use it, study it, modify it and share it for any noncommercial purpose (personal use, hobby projects, research, education, charities and public institutions). Selling it, or using it to make money, is not allowed without the author's permission; if you want a commercial license, open an issue.

### Third-party licenses

The bundled tools keep their own licenses:

- 7-Zip © Igor Pavlov — GNU LGPL + unRAR restriction (`src-tauri/bin/7zip-License.txt`).
- par2cmdline-turbo — GNU GPL v2 (https://github.com/animetosho/par2cmdline-turbo).

They run as separate programs and are extracted to `%LOCALAPPDATA%\Nebula\tools` the first time.
