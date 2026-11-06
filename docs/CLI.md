# The `nebula` command line

`nebula` controls a [nebula-server](SERVER.md) from the terminal: add NZBs, look at the queue and the history, pause, remove, limit the speed and search your indexers. `nebula get` also downloads an NZB on its own, without any server running, which is handy in scripts.

It comes in the Docker image (`docker exec nebula nebula status`) and in the [Release](../../../releases) as `nebula_<version>_linux_x64` and `_linux_arm64` (a static binary; rename it to `nebula` and put it in your `PATH`). Building it yourself gives `nebula-cli` (`nebula-cli.exe` on Windows, so it doesn't clash with the app's `nebula.exe`):

```bash
cargo build --release -p nebula-cli    # target/release/nebula-cli
```

## Connecting to the server

The commands that talk to a server need its address and Nebula's API key (**Settings → API**). The first value found wins:

1. `--url` and `--api-key`, or the variables `NEBULA_URL` and `NEBULA_API_KEY`.
2. `cli.toml` in the configuration folder (`~/.config/nebula/cli.toml`; `%APPDATA%\nebula\cli.toml` on Windows):

   ```toml
   url = "http://nas:5080"
   api_key = "0123456789abcdef0123456789abcdef"
   ```

3. Without a key, the settings of a nebula-server on the same machine: `NEBULA_DATA_DIR`, `/config` (Docker) or `~/.config/nebula`. That's why `docker exec nebula nebula …` works with no setup. The address defaults to `http://localhost:5080` (or the port in `NEBULA_PORT`).

nebula-server creates an API key on first start, so there's always one even while the API for Sonarr/Radarr is off.

## Commands

| Command | What it does |
|---|---|
| `nebula add <file.nzb or URL>… [--cat tv] [--priority high] [--via nntp] [--name N] [--paused]` | Adds to the queue. Priority: `low`, `normal`, `high`, `force`. `--via`: `nntp`, `debrid`, or the id of a server or debrid service. A category Nebula doesn't have goes to Default, with a warning. |
| `nebula queue` | The queue: position, short id, status, priority, progress, size, speed, time left, category and name. |
| `nebula history [-n 20] [--failed]` | The latest history entries (dates in UTC), with the error of the failed ones. |
| `nebula pause [ID]`, `nebula resume [ID]` | The whole queue, or one download. |
| `nebula rm <ID>… [--delete-files]` | Removes from the queue or from the history. |
| `nebula speed [LIMIT]` | Shows or sets the limit: `10` (MB/s), `2.5M`, `500K`, `1G`, `0` or `off`. |
| `nebula search <words>… [-i indexer] [--newznab 5000] [-n 25] [--grab N] [--cat C]` | Searches every enabled indexer (or the ones given) and lists the newest first. `--grab N` adds result number `N`; its category comes from its Newznab category unless `--cat` is given. |
| `nebula status` | Version, speed, queue, limit, free space, servers and missing tools. |
| `nebula tui` | The queue in the terminal, updated every second: arrows to choose, `p` pause/resume, space pause everything, `d` remove, `q` quit. |
| `nebula completions <shell>` | Completion script for `bash`, `zsh`, `fish`, `powershell` or `elvish`. |
| `nebula get <file.nzb> -o <folder>` | Downloads without a server (below). |

`ID` is a download's position in `nebula queue` (`1`, `2`…), its id or the start of its id (`5df4`). History entries go by id.

Every command accepts `--json` and then prints JSON instead of text: the same data the web interface gets (`queue`, `history`, `search`), or a short object (`{"added": [...]}`, `{"removed": [...]}`, `{"speedLimitKbps": 0}`…).

```bash
# Add every NZB in a folder to "movies"
nebula add ~/Downloads/*.nzb --cat movies

# Pause downloads at 8:00 and resume at 23:00 (crontab)
0 8 * * *  nebula pause
0 23 * * * nebula resume

# Name of what is downloading now
nebula queue --json | jq -r '.jobs[] | select(.status == "downloading") | .name'
```

## `nebula get`: downloading without a server

```bash
nebula get release.nzb -o ~/Downloads --server nntps://user:password@news.example.com:563?conns=20
```

It starts the engine inside the command with a temporary data folder, downloads the NZB into `<folder>/<name>`, verifies, repairs and extracts it, shows a progress bar and exits when it's done. Nothing stays behind except the download.

- `--server` (repeatable; the first one is the main one, the rest are backups): `nntps://` with TLS (port 563 by default) or `nntp://` without it (119). In the query: `conns=` connections, `verify=0` to accept any certificate, `name=`. Escape `@` and `:` in the user name or password as `%40` and `%3A`.
- Without `--server`, the servers of the Nebula on this machine are used (the desktop app or nebula-server, from the same folders as above).
- `--password` for the archives, `--no-pp` to skip verifying, repairing and extracting, `--verbose` to print the log, `--json` for a summary in JSON at the end.
- par2 and 7-Zip are the ones set in that Nebula's settings, the desktop app's, or the system's (`PATH`).

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Completed (warnings, if any, go to stderr). |
| `1` | Error: no servers, can't read the NZB, can't reach nebula-server, wrong API key… |
| `2` | Wrong usage: unknown option, bad priority or speed, ambiguous id. |
| `3` | The download failed (articles missing from every server, connection or login errors). |
| `4` | Verification or repair failed (not enough PAR2 blocks). |
| `5` | Extraction failed (wrong password, damaged archive…). |
| `130` | Cancelled with Ctrl+C. What was downloaded stays in the folder. |

The other commands exit with `0`, `1` or `2`.

## Language

Messages follow the system language (`LANG`/`LC_ALL`, or Windows' display language), in the same 12 languages as the app; `--help` is in English.
