# Categories

Every download in Nebula belongs to a **category** (`tv`, `movies`, `audio`…). The category decides where the download is saved, how it is downloaded, its priority in the queue and how much post-processing it gets. Categories are what Sonarr, Radarr and other programs use to keep their downloads apart, and they work the same way as in SABnzbd and NZBGet.

## Default

There is always a fixed **Default** category (stored as `*`, like in SABnzbd). It is exactly how Nebula worked before categories existed:

- It saves to the download folder from **Settings → Folders**.
- It uses the default download method from **Settings → NNTP / Debrid**.
- It uses the post-processing options from **Settings → Post-processing**.
- Its priority is **Normal**.

Downloads that were already in the queue or in History when you updated Nebula belong to Default; nothing else changes for them.

## Creating categories

Open **Settings → Categories** (on Android: **Settings → Categories**). Type a name and press **Add**, or press **Create tv, movies, audio and books** to start with the usual ones. Each category has:

| Option | Default | What it does |
|---|---|---|
| Name | — | Unique, not case-sensitive (`TV` and `tv` are the same category). It can't be `*`, `Default` or `None`, nor contain `/ \ " < > \| ? *`. |
| Folder | empty | Where its downloads are saved. Empty = the download folder; a relative folder (`tv`, `Series/Anime`) goes **inside** the download folder; an absolute one (`D:\Series`, `/mnt/media/tv`) is used as is. |
| Download with | Default method | NNTP (all servers or a specific one), a debrid service, or the default method. |
| Priority | Normal | Where its downloads enter the queue (see below). |
| Post-processing | As in the global settings | None, Repair, Repair and extract, or Repair, extract and delete (the same levels as SABnzbd's `pp` 0–3). |
| Newznab categories | empty | Newznab category numbers or ranges, separated by commas, whose search results go to this category on their own (see below). |

Removing or renaming a category doesn't touch what is already in the queue or History: those downloads keep the name they had.

Categories travel with **Settings → Move to another device**. A category that already exists on the other device keeps that device's folder; a new one keeps a relative folder and drops an absolute one (that folder probably doesn't exist there).

## Choosing the category of a download

| How the NZB arrives | How the category is chosen |
|---|---|
| **Add NZB** button, drag and drop, a link, the clipboard, a `.nzb` opened from Explorer, sharing to Nebula on Android | The add window has a **Category** list (shown once you have at least one category). Choosing a category fills in the folder, the download method, the post-processing switches and the priority; you can still change any of them for this download. |
| Search results | The category whose **Newznab categories** match the result's categories. With **Add with one click** on, the download goes straight to that category; otherwise the add window opens with it selected. |
| RSS rules | The rule's **Category**. Left on **By its Newznab category**, the item's Newznab categories decide. The rule's own folder and method, if set, win over the category's. |
| Phone controlling a PC | Search results sent to the PC use the **PC's** categories and mapping. NZBs added from **Downloads** while controlling a PC ask which of the PC's categories to use. |
| Phone or PC, add window hidden | With **Settings → Folders → Ask where to save** off, the add window isn't shown and the download goes to Default (or to the category matched by its Newznab categories when it comes from a search). |

The category of a queued download can be changed in its details (the files already downloaded stay where they are; the final folder doesn't change).

### Newznab category mapping

Indexers label each result with Newznab categories: `5000`–`5999` is TV (`5040` TV HD, `5070` anime…), `2000`–`2999` movies, `3000`–`3999` audio, `7000`–`7999` books. In a category's **Newznab categories** you can write single numbers and ranges:

```
5000-5999
2000-2999, 8010
```

If several categories match, the one with the **narrowest** range wins, so `anime` with `5070` beats `tv` with `5000-5999` for an anime release. A result that matches nothing goes to Default.

## Priority

The queue is ordered by priority. A new download goes after the ones with the same or higher priority and before the ones with lower priority. **Move to top** puts it first among those with its own priority, but never ahead of a higher one.

| Priority | Value | Behaviour |
|---|---|---|
| Force | `2` | First in the queue, and it downloads **even while the queue is paused**. |
| High | `1` | Ahead of Normal and Low. |
| Normal | `0` | The usual one. |
| Low | `-1` | After everything else. |

The values are the same as SABnzbd's, which the SABnzbd-compatible API uses. You can change a queued download's priority in its details (on Android: hold it → **Priority**), also from a phone controlling the PC.

## Where it shows

- **Downloads**: a pink chip with the category (Default has none) and, if it isn't Normal, a chip with the priority.
- **History**: the same chip, and a **category filter** next to the search box when History holds more than one category.
- **Log**: `Added to queue: Name [category]`.

## Data

- `config.json` → `categories`: list of `{ name, dir, via, priority, pp, newznab }`. `pp` is `-1` for "as in the global settings".
- `queue.json` → each job has `category` and `priority`.
- `history.json` → each entry has `category`.
- `config.json` → each RSS rule has `category` (`""` = by Newznab category, `"*"` = Default).

Files written by earlier versions have none of these fields; they load as Default with Normal priority, and the next save adds them. Older versions of Nebula ignore the new fields, so going back to an earlier version doesn't lose anything either.
