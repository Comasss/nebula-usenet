# How Nebula is put together

Nebula is a Cargo workspace with the engine on one side and the programs that host it on the other. The engine doesn't know whether it runs inside the Windows app, the Android app or the server: it asks the host for what depends on the platform and announces every change on an event bus.

```
crates/nebula-core   the engine, without Tauri
src-tauri            the Windows and Android app (Tauri) on top of nebula-core
crates/nebula-server the headless server (axum): the interface in the browser and the APIs
src                  the interface (Svelte), shared by every host
```

## nebula-core

Everything that downloads and processes: the queue and the NNTP workers (`engine.rs`, `nntp.rs`, `yenc.rs`), NZB parsing, post-processing with par2 and 7-Zip, categories, debrid services, indexers, RSS, actions on finish, statistics, the Newznab and SABnzbd APIs, remote control, moving settings to another device, settings and their files, the log and the engine's translations (`locales/`).

It depends on tokio, reqwest and the like, never on Tauri; CI checks that `cargo build -p nebula-core` works and that `cargo tree -p nebula-core` has no `tauri`.

| Module | What it is |
|---|---|
| `engine.rs` | `Engine`: the queue, the history and the workers. Created with `Engine::new(platform, paths, settings)` and started with `start()`. |
| `facade.rs` | What the interfaces ask for that isn't a plain queue call (preview an NZB, search an indexer, save an NZB, test an action…). The app's commands are one-line wrappers around it, and the server and the CLI will use the same methods. |
| `bus.rs` | The event bus (below). |
| `platform.rs` | The `Platform` trait (below) and `Headless`, the host with no interface. |
| `rt.rs` | `rt::spawn`: launches a task on the host's tokio runtime (`rt::set_handle`) or on one of its own if there is none. |
| `secrets.rs` | How passwords and API keys are stored in `config.json`: DPAPI on Windows, base64 on Android (the file lives in the app's private folder), ChaCha20-Poly1305 elsewhere with the key in `<data>/secret.key` (600). |
| `tools.rs` | Where par2 and 7-Zip are: bundled in the `tools` folder on Windows, inside the APK on Android, and elsewhere the paths in the settings (`par2Path`, `sevenZipPath`) or the system ones (`par2`, `7zz`/`7z`/`7za`) found in `PATH`. |
| `sys.rs` | Free disk space on each system. |

Code that only exists on one system stays in the same file behind `#[cfg(windows)]`, `#[cfg(unix)]` or `#[cfg(target_os = "android")]`.

## The event bus

The engine announces changes with `bus::emit(Event::…)`; whoever shows them subscribes with `bus::subscribe()`. It's a tokio `broadcast` channel: a slow listener loses the oldest events (it gets `Lagged`) instead of slowing the engine down.

`Event` is a serializable enum. `name()` gives the name the interface listens to and `payload()` its data as JSON, the same names and data the app used to send directly:

| Event | Data |
|---|---|
| `queue` | The queue snapshot (`Snapshot`), at most twice a second and only when something changed. |
| `history` | — (the interface asks for the history again). |
| `log` | A log line. |
| `toast` | `{kind, message}` for a notice in the interface. |
| `open-items` | `.nzb` files or links to open in the add window. |
| `clipboard-nzb` | NZB links found in the clipboard. |
| `settings` | The whole settings, when the engine changes them (quick power off, a one-time action). |
| `rss` | — (the RSS state changed). |
| `api-status` | Whether the API is listening and on which addresses. |
| `power-countdown`, `power-cancelled` | The power-off countdown. |
| `transfer-received`, `transfer-closed` | Settings arriving from another device. |
| `remote-status`, `remote-devices`, `remote-pair-request`, `remote-pair-closed` | The PC side of remote control. |
| `remote-pair-code`, `remote-state`, `remote-queue`, `remote-history` | The phone side of remote control. |

Serialized, an event is `{"event": "<name>", "payload": …}`.

## Platform

What the engine needs from its host and depends on the system goes through the `Platform` trait. Every method has a default that does nothing, so a host only implements what it has:

| Method | In the app |
|---|---|
| `notify` | Windows notification; Android notification. |
| `notify_power` | Windows notification with a Cancel button for the power-off countdown. |
| `queue_status` | Tray tooltip on Windows; the persistent notification that keeps downloads alive on Android. |
| `tray_paused`, `refresh_power` | Texts and check marks of the tray menu. |
| `show_main` | Shows the main window. |
| `power_window` | Opens or closes the small countdown window. |
| `supports_power`, `power_action` | Shut down, sleep or hibernate (Windows). |
| `set_association`, `set_autostart` | Open `.nzb` files with Nebula and start with Windows. |

## The app (src-tauri)

`src-tauri` keeps only what needs Tauri: windows, the tray, the commands the interface calls, the Windows integration (`winint.rs`: registry, notifications, clipboard), the Android plugin (`android.rs`) and the updater. On start it:

1. Gives the engine Tauri's runtime (`rt::set_handle`).
2. Subscribes to the bus and forwards every event to the interface with `app.emit(event.name(), event.payload())`.
3. Creates the engine with `TauriPlatform` (`os.rs`), its `Platform` implementation.

The package is still called `nebula`, so the executable is still `nebula.exe` and the Android library `libnebula_lib.so`.

## The server (crates/nebula-server)

A binary with axum on top of nebula-core, with `Headless` as its platform. See [SERVER.md](SERVER.md) for how to use it.

| File | What it is |
|---|---|
| `main.rs` | Options (clap), the data folder, starting the engine, and stopping cleanly on SIGTERM/Ctrl+C. |
| `auth.rs` | The web user: argon2 hash and HMAC-signed session cookies in `auth.json`. |
| `rpc.rs` | `POST /api/v1/invoke/<command>`: the app's command names mapped to the same engine and facade calls. |
| `http.rs` | The routes: sign in, commands, events over WebSocket and SSE, the Newznab/SABnzbd APIs (`api::handle_http`, the same handler as the app's API port) and the interface embedded with rust-embed. |

## The interface

Components talk to the engine only through `src/lib/transport.js`: `invoke(command, args)` and `listen(event, handler)`. Inside the app it's Tauri's IPC; in a browser (nebula-server) it's `fetch` to `/api/v1/invoke/…` and one shared WebSocket for the events, which reconnects by itself and then sends a `reconnected` event so the interface reloads the queue and the settings.

`src/lib/platform.js` says where the interface runs (`isDesktopApp`, `isAndroid`, `isServer`, `isMobile`), and components use it to hide what doesn't apply: window controls, tray, clipboard, updater and opening files exist only in the app; the phone layout is used on Android and in a narrow browser.

## Data

Splitting the engine out of the app didn't change any file: `config.json`, `queue.json`, `history.json` and the rest are read and written exactly as before, in the same folder (`%APPDATA%\Nebula` on Windows, the app's private folder on Android, or `NEBULA_DATA_DIR`). nebula-server uses the same files in its own data folder, plus `auth.json` and, outside Windows, `secret.key`.
