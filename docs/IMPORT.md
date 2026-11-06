# Moving from SABnzbd or NZBGet

Nebula reads the configuration file of SABnzbd (`sabnzbd.ini`) or NZBGet (`nzbget.conf`) and adds its servers, categories, folders, options and scheduled tasks. It can also take the indexers from Prowlarr or NZBHydra2. Nothing is added until you confirm, and what you already have is never changed or duplicated.

| From | Where |
|---|---|
| [`sabnzbd.ini` or `nzbget.conf`](#importing-a-configuration-file) | Settings → Import, or `nebula import <file>` |
| [Prowlarr or NZBHydra2](#indexers-from-prowlarr-or-nzbhydra2) | Indexers → From Prowlarr / NZBHydra2 |

## Importing a configuration file

Where to find the file:

| | Linux / Docker | Windows |
|---|---|---|
| SABnzbd | `~/.sabnzbd/sabnzbd.ini`, or `/config/sabnzbd.ini` in Docker | `%LOCALAPPDATA%\sabnzbd\sabnzbd.ini` |
| NZBGet | `~/.nzbget`, `/etc/nzbget.conf`, or `/config/nzbget.conf` in Docker | `nzbget.conf` in NZBGet's folder |

In **Settings → Import**, choose the file. A window shows everything Nebula found, with a switch for each server, category and setting, and one for all the scheduled tasks. Servers and categories you already have appear as **Already exists** and are left alone. Above the list, a note explains whatever can't be imported.

From the terminal, against a nebula-server:

```bash
nebula import /config/sabnzbd.ini --dry-run   # only show what would be imported
nebula import /config/sabnzbd.ini             # add everything
```

`nebula import` adds everything it finds (servers and categories that already exist are skipped). With `--json` it prints the preview and the result.

A server already exists when it has the same host, port and user. A category already exists when it has the same name, ignoring case. Importing the same file twice adds nothing new.

### What is imported

| | SABnzbd | NZBGet |
|---|---|---|
| Servers | Host, port, SSL and certificate check, user and password, connections, priority, retention, enabled. A server's `quota` becomes a quota with no reset. | `ServerN.*`: host, port, encryption, certificate check, user and password, connections, level (priority), retention, active. |
| Categories | Name, folder, priority, post-processing level and Newznab categories (`5000-5999`). The `*` category is skipped: Nebula has its own Default. | `CategoryN.*`: name and folder. `Unpack=no` becomes the "repair only" level. |
| Folders | `complete_dir` → download folder, `download_dir` → temporary folder. | `DestDir` → download folder, `InterDir` → temporary folder. `${MainDir}` and other variables are expanded. |
| Options | Pre-check, direct unpack, deobfuscation, PAR2 cleanup, extraction, unwanted extensions and their action, duplicate handling. | Speed limit (`DownloadRate`), PAR2 check, extraction, deleting archives, direct unpack, health check (cancel hopeless downloads), duplicate check. |
| Scheduler | Pause, resume and speed limit tasks. A percentage limit is turned into KB/s with `bandwidth_max`. | `TaskN.*` with `PauseDownload`, `UnpauseDownload` and `DownloadRate`, one Nebula task per time. |

Settings import as they are, so the folders keep the paths of the machine SABnzbd or NZBGet ran on. Check them in **Settings → Folders** if Nebula runs elsewhere, or in Docker with different volumes.

### What isn't imported

The import window lists these when the file has them:

- **Relative folders** (`Downloads/complete`): choose the folder in Settings.
- **The password file**: copy the passwords into **Settings → Post-processing → Fallback passwords**.
- **A global quota**: in Nebula each server has its own (see [quotas](DOWNLOADING.md#download-quota-per-server)).
- **RSS feeds, scripts and extensions**, and scheduled tasks other than pause, resume and speed (RSS scans, enabling or disabling servers, `*:00` times in NZBGet…).

Duplicate handling is translated to the closest option: SABnzbd's "Discard" and "Fail job" become **Don't add**, "Pause" becomes **Add paused** and "Tag job" becomes **Add and warn**. NZBGet's `DupeCheck=yes` becomes **Add and warn**.

## Indexers from Prowlarr or NZBHydra2

In **Indexers → From Prowlarr / NZBHydra2**, type the address and API key (in Prowlarr: Settings → General; in NZBHydra2: Config → Main) and press **Find indexers**.

- **Prowlarr**: each Usenet indexer is added on its own, through Prowlarr's Newznab proxy (`http://prowlarr:9696/1/api`), so Nebula shows which one each result comes from. Torrent indexers are left out, and indexers disabled in Prowlarr come in disabled.
- **NZBHydra2** (or any Newznab address): it's added as a single indexer that searches all of NZBHydra2's.

Indexers with the same address as one you have appear as **Already exists**. The same is also in **Settings → Import**.
