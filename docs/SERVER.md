# nebula-server (Linux, NAS, headless)

> For Docker, Unraid and Synology, see [DOCKER.md](DOCKER.md).

`nebula-server` is Nebula without a window: the same engine as the desktop and Android apps, with the interface in the browser and the Newznab and SABnzbd APIs on the same port. It's meant for a server, a NAS or a container, next to Sonarr and Radarr.

```
nebula-server --data-dir ~/.config/nebula --port 5080
```

Open `http://<server>:5080` in a browser. The first time, Nebula asks you to create a user and a password (8 characters at least); from then on it asks you to sign in. After that the first-run wizard is the same as in the app: pick your features and add your servers and indexers.

## Options

Every option can also be given as an environment variable, which is handier in Docker and systemd.

| Option | Variable | Default | What it does |
|---|---|---|---|
| `--data-dir` | `NEBULA_DATA_DIR` | `/config` in Docker, `~/.config/nebula` elsewhere (`%APPDATA%\Nebula Server` on Windows) | Settings, queue, history, logs, `auth.json` and `secret.key`. |
| `--port` | `NEBULA_PORT` | `5080` | Port for the interface and the APIs. |
| `--bind` | `NEBULA_BIND` | `0.0.0.0` | Address to listen on. `127.0.0.1` keeps it to this machine (behind a reverse proxy, for example). |
| `--download-dir` | `NEBULA_DOWNLOAD_DIR` | `~/Downloads/Nebula` (`/downloads` in Docker) | Default download folder, used until you choose another one in **Settings → Folders**. |

`nebula-server --help` lists them, and `--version` prints the version. `nebula-server healthcheck` exits with 0 if the server on that port answers `/health` (Docker uses it).

## What's different from the app

The interface is the same, minus what only makes sense with a window on your own computer:

- No window bar, tray, start with Windows, `.nzb` file association, clipboard watching, shutdown after finishing or built-in updater. Update the binary or the container image instead.
- Folders are picked with Nebula's own folder browser, which shows the **server's** folders, not the ones on the computer you're browsing from.
- `.nzb` files are uploaded from the browser: drag them onto the page or use **Add NZB**.
- Opening a finished download in a file manager isn't possible; History shows its path.
- On a narrow screen (a phone) the interface switches to the phone layout, with the floating tab bar.
- **Settings → Web account** shows who is signed in, signs out and changes the password.

## Signing in

`auth.json` in the data folder holds the user name, the password's argon2 hash and the key that signs the session cookies. It's written with permissions `600`. A session lasts 30 days in each browser and survives restarts; changing the password signs out every session.

Forgot the password? Stop Nebula, delete `auth.json` and start it again: it asks you to create the user again. Nothing else is lost.

Scripts and the future CLI can call the interface's commands without a cookie by sending Nebula's API key (from **Settings → API**) in an `X-Api-Key` header. That works even before the web user exists.

## Sonarr, Radarr and Prowlarr

The [Newznab](API.md) and [SABnzbd](SABNZBD.md) APIs answer on the same port as the interface, at `/api` and `/sabnzbd/api`, with the API key from **Settings → API** (turn on **Enable API** there first). In Sonarr or Radarr add a **SABnzbd** download client with the server's host, port `5080`, an empty URL Base (or `/sabnzbd`) and that key. **Settings → API** shows the exact values.

The app's separate API port (`5077`) isn't used by nebula-server: everything goes through `--port`.

## par2 and 7-Zip

Repair and extraction use the system's `par2` and `7zz`/`7z`/`7za`, found in `PATH` (or in `<data-dir>/tools`). For speed, [par2cmdline-turbo](https://github.com/animetosho/par2cmdline-turbo) is recommended. On Debian/Ubuntu:

```
sudo apt install par2 7zip
```

If one is missing, **Settings → Post-processing** says so and lets you type the full path of each program; leave the field empty to find it automatically. Without par2 damaged downloads aren't repaired, and without 7-Zip archives aren't extracted.

## Passwords and API keys

On Linux and macOS, passwords and API keys in `config.json` are encrypted with ChaCha20-Poly1305. The key is `secret.key` in the data folder, created with permissions `600` the first time something is encrypted. Keep it with `config.json` when you move or back up the data folder: without it the passwords have to be typed again. Settings written by an older version (base64) are still read and get encrypted the next time they are saved.

## Stopping

`Ctrl+C`, `docker stop` and `systemctl stop` (SIGINT or SIGTERM) stop Nebula cleanly: it stops accepting connections, gives open browser tabs up to 3 seconds, saves the queue and the history and exits. Downloads in progress continue where they were on the next start.

## The HTTP interface

What the browser uses, also useful for scripts:

| Route | What it is |
|---|---|
| `GET /health` | `{"status": "ok", "version": "…"}`, without signing in. For Docker's healthcheck and monitors. |
| `GET /api/v1/auth` | `{needsSetup, user, minPassword, version}`. |
| `POST /api/v1/auth/setup` | `{username, password}`. Creates the user (only the first time) and signs in. |
| `POST /api/v1/auth/login` | `{username, password}`. Sets the session cookie. |
| `POST /api/v1/auth/logout` | Clears the cookie. |
| `POST /api/v1/invoke/<command>` | The interface's commands, with the same names and arguments (camelCase JSON) as in the app: `get_queue`, `pause_job` `{"id": …}`, `set_speed_limit` `{"kbps": …}`… `import_nzb` takes the raw `.nzb` as the body and its name in an `X-Name` header. Errors come as `{"error": "…"}`. |
| `GET /api/v1/events` | WebSocket with every engine event as `{"event": "<name>", "payload": …}` (see [ARCHITECTURE.md](ARCHITECTURE.md#the-event-bus)). |
| `GET /api/v1/events/sse` | The same events as Server-Sent Events. |
| `/api`, `/sabnzbd/api`, `/newznab/api` | The Newznab and SABnzbd APIs. |

Everything under `/api/v1/` except `/health` and `/api/v1/auth*` needs the session cookie or `X-Api-Key`. Request bodies are limited to 100 MB.

## Installing as a service (systemd)

The [Release](../../../releases) has `nebula-server_<version>_linux_x64` and `_linux_arm64`: one static binary that runs on any Linux distribution. [`deploy/nebula-server.service`](../deploy/nebula-server.service) runs it as its own `nebula` user with its data in `/var/lib/nebula`; the steps to install it are at the top of the file.

## Building

```
npm ci && npm run build          # the interface, into dist/
cargo build --release -p nebula-server
```

The interface in `dist/` goes inside the binary (`target/release/nebula-server`), so it's a single file. In a debug build it's read from `dist/` on every request, so `npm run build` is enough to see interface changes.
