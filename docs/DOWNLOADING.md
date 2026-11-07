# Download options

Options that decide how Nebula downloads and post-processes, beyond servers and categories. They are the same in the Windows app, on Android, in nebula-server and in Docker. Everything is on by default except the pre-check and extracting while downloading, and settings from earlier versions load unchanged.

| Option | Where | Default |
|---|---|---|
| [Check before downloading](#check-before-downloading) | Settings → Connection | Off |
| [Duplicate downloads](#duplicate-downloads) | Settings → Connection | Add and warn |
| [Deobfuscate file names](#deobfuscate-file-names) | Settings → Post-processing | On |
| [Extract while downloading](#extract-while-downloading) | Settings → Post-processing | Off |
| [Fallback passwords](#fallback-passwords) | Settings → Post-processing | None |
| [Unwanted files](#unwanted-files) | Settings → Post-processing | Warn, with `exe scr com bat cmd lnk vbs msi ps1 jar` |
| [Scheduler](#scheduler) | Settings → Scheduler | No tasks |
| [Download quota](#download-quota-per-server) | Servers → edit a server | No quota |

## Check before downloading

Before downloading anything, Nebula asks the server about every article with `STAT`, which only checks that it exists. It uses the same connections as the download, so it doesn't need extra connections from your provider. If an article is missing from the main server, the backup servers are asked, in priority order.

- As soon as more is missing than the PAR2 recovery files can repair (or something is missing and there are no PAR2 files), the download is canceled and goes to History as failed with the reason. **Nothing has been downloaded** by then.
- If it can be completed, the download starts. Articles that the check already knows are missing are not requested again, and articles missing only from the main server go straight to a backup server.
- While it checks, the download shows **Checking** with its own progress. Sonarr and Radarr see the SABnzbd status `Checking`.

It takes a moment at the start (a few seconds for a typical episode), so it's off by default. It's most useful with old posts, blocks that are often taken down, or a metered connection. Without it, **Cancel hopeless downloads** still stops a download midway when it can no longer be repaired.

## Duplicate downloads

A new NZB counts as a duplicate when a download with the same name (ignoring case, dots, dashes and spaces) or the same NZB content (the first article of each file, even from a different indexer) is in the queue or in History as completed. Failed downloads don't count, so you can retry them.

| Option | What happens |
|---|---|
| Add anyway | Nothing, as before. |
| Add and warn | It's added with a warning on the download and in the log. |
| Add paused | It's added paused, with the warning. |
| Don't add | It's rejected with "Already queued or downloaded". Sonarr and Radarr get the error through the SABnzbd API. |

`nebula add --force` adds it whatever this option says.

## Deobfuscate file names

Many posts use random names (`a8f3c1….mkv`, or no extension at all) so that they are harder to take down. Nebula fixes them in three steps:

1. While downloading, a file takes the name in its yEnc header when it's better than the NZB's.
2. Before verifying, the `.par2` file is read. It lists every file's real name, size and the MD5 of its first 16 KB, so files that match get their real name back.
3. At the end, if the largest file still has a random name and is clearly the main file (at least twice the size of the next one), it takes the download's name, and so do its companions with the same name (`.srt`, `.nfo`…). Season packs with several similar files are left alone.

## Extract while downloading

Each archive (RAR, 7z, ZIP or split volumes) is extracted as soon as all of its volumes are downloaded without errors, while the rest of the download continues. Season packs and other downloads with several archives finish sooner. The cost is extra disk and CPU work during the download.

- One archive is extracted at a time. If one fails (a missing volume, a wrong password), it's tried again at the end as usual.
- If the PAR2 check then has to repair anything, every archive is extracted again at the end, so a damaged file never stays.
- It needs **Extract archives** on. Off by default.

7-Zip only extracts an archive once all its volumes are there, so a download with a single archive gains nothing.

## Fallback passwords

If an archive is protected and the download's password (from the NZB, the `{{password}}` in the name or the one you typed) doesn't open it, Nebula tries these passwords in order. The log says when a fallback password worked. If none works, the download fails with "Wrong password" and you can retry the extraction from History with another one.

## Unwanted files

Files with these extensions are checked twice: in the NZB when it's added, and among the extracted files at the end. Type them without the dot; **Restore** brings back the default list.

| Action | In the NZB | After extracting |
|---|---|---|
| Do nothing | — | — |
| Warn | The download gets a warning. | History shows a warning. |
| Discard them | Those files aren't downloaded. | They are deleted. |
| Reject the download | It isn't added ("Contains unwanted files"). | The download fails. |

## Scheduler

Tasks that pause the queue, resume it or change the speed limit at a time of day, on the days you mark (none marked means every day). They use the local time of the machine that runs Nebula. In Docker that's the `TZ` variable (for example `TZ=Europe/Madrid`); without it, UTC.

- The tasks are checked every minute, so a task at 08:00 runs between 08:00:00 and 08:00:59.
- On start, Nebula applies what the latest tasks of the past week left behind. If the queue should be paused since 08:00 and Nebula starts at 10:00, it starts paused.
- You can still pause, resume or change the limit by hand; the next task changes it again.
- A speed limit of 0 removes the limit.

## Download quota per server

Some providers sell blocks of data or limit how much you can download. Each server can have a quota in GB and when it resets: every day, every week (on the weekday you choose), every month (on day 1–28) or never.

- What each server downloads is counted and saved in `quota.json` in the data folder, so it survives restarts.
- When a server reaches its quota, it stops being used until the next period. Downloads go on with the other servers; if there are none left, they wait.
- **Servers** shows how much of the quota is used, the **Quota used up** state and a **Reset** button that sets the count back to zero.
- Periods start at 00:00 local time on the day they reset.
