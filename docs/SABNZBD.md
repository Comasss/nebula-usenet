# SABnzbd-compatible API (download client)

Nebula speaks the part of the [SABnzbd API](https://sabnzbd.org/wiki/configuration/4.5/api) that Sonarr, Radarr, Lidarr, Readarr, Prowlarr and similar programs use. In those programs you add Nebula as a **SABnzbd** download client: they send it NZBs, follow the download in the queue and, when History says it's completed, import it from the final folder.

It runs on the same server, port and API key as the [Newznab API](API.md): requests with `mode=` are SABnzbd, requests with `t=` are Newznab.

## Setting it up

1. In Nebula, open **Settings → API**, turn on **Enable API** and, if Sonarr/Radarr run on another computer, a NAS or in Docker, **Allow connections from the local network**.
2. In **Settings → Categories**, create the categories the programs will use (`tv` for Sonarr, `movies` for Radarr…). Like in SABnzbd, a category Nebula doesn't know goes to Default, and Sonarr's **Test** warns about it.
3. In Sonarr or Radarr: **Settings → Download Clients → + → SABnzbd**

   | Field | Value |
   |---|---|
   | Host | `localhost`, or the computer's IP shown in **Settings → API** |
   | Port | `5077` (or the port you set) |
   | URL Base | empty (`/sabnzbd` also works) |
   | API Key | the key from **Settings → API** |
   | Category | `tv`, `movies`… (one that exists in Nebula) |

   Press **Test**, then **Save**.

Sonarr imports from the folder Nebula reports. If both run on the same machine, nothing else is needed. If Sonarr runs elsewhere (Docker, another PC), the download folder must be reachable from there under the same path, or you need a **Remote Path Mapping** in Sonarr.

## Supported modes

Every call except `version` and `auth` needs `apikey=` (or `nzbkey=`, which takes the same key). Answers are always JSON (`output=json` is accepted and ignored). Requests can be `GET` or `POST`; parameters can go in the URL, in a `application/x-www-form-urlencoded` body or in a `multipart/form-data` body.

| Mode | Parameters | Answer |
|---|---|---|
| `version` | — | `{"version": "4.5.1"}` |
| `auth` | — | `{"auth": "apikey"}` |
| `get_config` | `section` (optional) | `config.misc` (`complete_dir`, `download_dir`, `version`, `pre_check`, sorting off, history retention…) and `config.categories` (`name`, `order`, `dir`, `pp`, `script`, `priority`, `newzbin`). |
| `get_cats` | — | `{"categories": ["*", "tv", "movies"]}` |
| `get_scripts` | — | `{"scripts": ["None"]}` |
| `addfile` | multipart file in the field `name` or `nzbfile`; `cat`, `priority`, `pp`, `nzbname` | `{"status": true, "nzo_ids": ["SABnzbd_nzo_…"]}` |
| `addurl` | `name` = URL of the `.nzb`; `cat`, `priority`, `pp`, `nzbname` | same as `addfile` (Nebula fetches the NZB before answering) |
| `queue` | `start`, `limit`, `category`/`cat`, `search`, `nzo_ids` | `queue` with `paused`, `status`, `kbpersec`, `speedlimit`, `speedlimit_abs`, `mb`, `mbleft`, `timeleft`, `noofslots`, `diskspace1` and `slots` (`index`, `nzo_id`, `filename`, `cat`, `status`, `priority`, `mb`, `mbleft`, `size`, `sizeleft`, `percentage`, `timeleft`, `missing`…) |
| `queue&name=delete` | `value` = ids separated by commas or `all`; `del_files` | `{"status": true, "nzo_ids": [...]}` |
| `queue&name=pause` / `resume` | `value` = ids (without `value`: the whole queue) | `{"status": true, ...}` |
| `queue&name=priority` | `value` = id, `value2` = priority | `{"position": n}` |
| `queue&name=change_cat`, `change_cat` | `value` = id, `value2` = category | `{"status": true}` |
| `pause` / `resume` | — | pauses or resumes the whole queue |
| `config&name=speedlimit` | `value` | `{"status": true}` (see the differences below) |
| `history` | `start`, `limit`, `category`/`cat`, `failed_only`, `search`, `nzo_ids` | `history` with `noofslots`, `total_size`, `ppslots` and `slots` (`nzo_id`, `name`, `nzb_name`, `category`, `status`, `fail_message`, `storage`, `path`, `bytes`, `size`, `download_time`, `completed`, `retry`…) |
| `history&name=delete` | `value` = ids, `all`, `failed` or `completed`; `del_files` | `{"status": true}` |
| `retry` | `value` = id of a history entry | `{"status": true, "nzo_id": "…"}` (a new id) |
| `fullstatus` / `status` | — | minimal: `version`, `paused`, `completedir`, `downloaddir`, `kbpersec`, `speedlimit_abs` |
| `warnings` | — | `{"warnings": []}` |
| `server_stats` | — | zeros |

Anything else answers `{"status": false, "error": "not implemented"}`. A missing key answers `{"status": false, "error": "API Key Required"}` and a wrong one `"API Key Incorrect"`, the texts Sonarr shows as an authentication error.

### Priorities and post-processing

| `priority` | Meaning |
|---|---|
| `-100` or missing | The category's priority |
| `-2` | Added paused |
| `-1` / `0` / `1` | Low / Normal / High |
| `2` | Force: downloads even while the queue is paused |

`pp`: `0` nothing, `1` repair, `2` repair and extract, `3` repair, extract and delete the archives and `.par2` files. Missing: the category's level (or the global settings for Default). The queue shows the priority as text (`Force`, `High`, `Normal`, `Low`), like SABnzbd.

### Job ids

`nzo_id` is `SABnzbd_nzo_` followed by Nebula's own id for the download. It stays the same across restarts, and the history entry keeps the id the download had in the queue, so Sonarr can match both. Ids are accepted with or without the prefix. A **retry** creates a new id, as in SABnzbd.

## How Nebula's states map to SABnzbd

Like SABnzbd, a download leaves the queue when its post-processing starts and shows up in History with the post-processing state; Sonarr expects it there and imports it when it becomes `Completed`.

| Nebula | Where | SABnzbd `status` |
|---|---|---|
| Queued | queue | `Queued` |
| Paused | queue | `Paused` |
| Downloading (NNTP) | queue | `Downloading` |
| Debrid: sending the NZB to the service | queue | `Grabbing` |
| Debrid: the service is downloading it | queue | `Downloading` (with the service's progress and time left) |
| Debrid: downloading the files to this device | queue | `Downloading` |
| Verifying (PAR2) | history | `Verifying` |
| Repairing | history | `Repairing` |
| Extracting | history | `Extracting` |
| Moving | history | `Moving` |
| Completed | history | `Completed`; `storage` is the final folder (with the job's subfolder if **Create subfolder** is on) |
| Failed | history | `Failed`; `fail_message` is Nebula's error, `storage` the temporary folder |

The whole queue reports `Paused` when it's paused, `Downloading` when something is downloading and `Idle` otherwise.

## Differences from SABnzbd

- **One key.** `nzbkey` is accepted but it is the same key as `apikey`, with the same permissions.
- **JSON only.** `output=xml` isn't supported.
- **`addurl` waits** until Nebula has fetched the `.nzb` (SABnzbd answers first and fetches later). A link that doesn't return an NZB answers `status: false` with the reason.
- **Speed limit.** Nebula has no "maximum line speed", so `config&name=speedlimit` takes absolute values: `500K`, `2.5M`, `1G`, or a plain number in KB/s. `0`, `100`, `100%` or an empty value remove the limit; other percentages are rejected.
- **History retention.** `get_config` reports `history_retention_option: "all"`, but Nebula keeps the newest entries up to **Settings → Connection → History limit** (300 by default). Keep it high enough for Sonarr to import before entries are dropped.
- **Changing the category** of a queued download (`change_cat`) changes its label, not the folder it was going to (SABnzbd moves it).
- **Not available:** RSS, scheduler, servers, sorters, scripts (the `script` parameter is ignored; use **Settings → On finish**), `addlocalfile`, `switch` (reordering by position; Nebula orders by priority), `get_files`, `shutdown`/`restart`.
- **Debrid.** Downloads through TorBox or Premiumize appear in the queue like any other; their categories can choose debrid as the method (**Settings → Categories → Download with**).

## Testing it

`dev/sonarr_sim.py` repeats Sonarr's sequence against a running Nebula (version → get_config → addfile → queue → history until `Completed`):

```bash
python dev/mock_nntp.py --src folder --nzb test.nzb --kbps 600
# Nebula with the mock server, the API on and a "tv" category
python dev/sonarr_sim.py --nzb test.nzb --key <API key> --cat tv [--base /sabnzbd]
```

`cargo test sonarr_sequence` runs the same sequence through Nebula's HTTP parser against a simulated engine.
