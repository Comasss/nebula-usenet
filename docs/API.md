# Nebula API (Newznab / NZBHydra2 compatible)

Nebula can expose all the Newznab indexers you have configured as **one Newznab indexer**, the same way [NZBHydra2](https://github.com/theotherp/nzbhydra2) does. Sonarr, Radarr, Lidarr, Readarr, Prowlarr, Mylar or any other program that speaks Newznab can then search every indexer at once through Nebula with a single URL and API key.

- One search goes to all your enabled indexers in parallel; results are merged, sorted by date and (if **Settings → Indexer search → Remove duplicates** is on) the same release found on several indexers is returned once.
- Download links point to Nebula, which fetches the `.nzb` from the indexer. Your indexers' API keys never leave Nebula.
- Searches and grabs made through the API count in **Statistics → Indexers** like the ones made in the app.
- The same server, port and key also speak the **SABnzbd API**, so the same programs can use Nebula as their download client. See [SABNZBD.md](SABNZBD.md).

## Turning it on

1. Add at least one indexer in **Indexers** (the API searches the enabled ones).
2. Open **Settings → API** and turn on **Enable API**. An API key is generated the first time.
3. Copy the address shown there (for example `http://localhost:5077/api`) and the API key.

Options in **Settings → API**:

| Option | Default | What it does |
|---|---|---|
| Enable API | off | Starts or stops the API server. |
| API key | random | Programs must send it with every request. **Generate new** replaces it (programs using the old one stop working until you update them). |
| Port | `5077` | TCP port the API listens on. NZBHydra2 uses 5076, so both can run on the same machine. |
| Allow connections from the local network | off | Off: only programs on the same computer can connect (`127.0.0.1`). On: other devices on your network (a NAS, another PC…) can connect using the computer's IP, shown as a second address. |

If the port is taken by another program, Settings shows the error and the API stays off until you pick another port.

On **Android** the API works the same way, but Android may stop Nebula when it is in the background and not downloading, so the API only answers reliably while the app is open or busy.

## Connecting programs

### Sonarr / Radarr / Lidarr / Readarr

**Settings → Indexers → + → Newznab → Custom**

- **URL**: `http://localhost:5077` (or `http://<computer IP>:5077` from another device)
- **API Path**: `/api`
- **API Key**: the key from Nebula
- **Categories**: pick the ones you want (Nebula offers the standard Newznab list).

Press **Test**, then **Save**.

### Prowlarr

**Indexers → Add Indexer → Generic Newznab**, with the same URL, `/api` path and API key. Prowlarr will then sync it to your *arr apps.

### Anything else

Any client that lets you add a "Newznab" or "custom Newznab" indexer works with the URL `http://<host>:5077/api` and the API key.

## Reference

Base URL: `http://<host>:<port>/api` (`/newznab/api` is accepted too). Every request is a `GET` (or `HEAD`) with query parameters. The API key goes in `apikey=` or in an `X-Api-Key` header. Responses are XML in the Newznab format.

### `t=caps`

Capabilities: server info, limits (default 100, max 500 results), supported searches and the standard Newznab category tree (1000 Console, 2000 Movies, 3000 Audio, 4000 PC, 5000 TV, 6000 XXX, 7000 Books, 8000 Other, with subcategories). It is the only function that works without the API key.

```
GET /api?t=caps
```

### Searches

| `t=` | Aliases | Parameters passed to the indexers |
|---|---|---|
| `search` | | `q` |
| `tvsearch` | `tv-search` | `q`, `season`, `ep`, `imdbid`, `tvdbid`, `rid`, `tvmazeid` |
| `movie` | `movie-search` | `q`, `imdbid`, `tmdbid` |
| `music` | `audio`, `audio-search` | `q`, `artist`, `album` |
| `book` | `book-search` | `q`, `author`, `title` |

Common parameters:

| Parameter | Meaning |
|---|---|
| `apikey` | Nebula's API key (required). |
| `cat` | Comma-separated category ids, e.g. `5030,5040`. |
| `limit` | Results per indexer, 1–500 (default 100). |
| `offset` | Page offset, passed to every indexer. |
| `maxage`, `minsize`, `maxsize` | Passed to the indexers that support them. |
| `indexers` | Optional. Comma-separated indexer names (as shown in Nebula) or ids to search only those, e.g. `indexers=NZBGeek,NZBFinder`. |

Other Newznab parameters (`traktid`, `label`, `track`, `year`, `genre`) are passed through as well. If an indexer doesn't support a TV/movie/music/book search and there is a `q`, Nebula repeats it there as a plain text search.

Example:

```
GET /api?t=tvsearch&q=Some+Show&season=1&ep=2&cat=5040&apikey=YOUR_KEY
```

Each `<item>` has the usual Newznab fields: `title`, `guid`, `link` and `enclosure` (Nebula's download link), `comments` (the indexer's details page), `pubDate`, `category`, and the attributes `category`, `size`, `grabs`, `files`, `password`, `usenetdate` and `hydraIndexerName` (the indexer the result came from). `newznab:response total` is the sum of the totals reported by the indexers.

If some indexers fail, the results of the others are returned and the failure is written to Nebula's log. If all of them fail, the response is an error with code `900` and each indexer's message.

### `t=get`

Downloads the `.nzb` of a search result. You don't build this URL yourself: it is the `link` of each result.

```
GET /api?t=get&id=<id>&apikey=YOUR_KEY
```

The response is the `.nzb` (`application/x-nzb`) with its file name in `Content-Disposition`. Ids are remembered in memory for the last 20,000 results; after restarting Nebula, search again before grabbing an old result.

### Errors

Errors use the Newznab format, `<error code="…" description="…"/>`:

| Code | When |
|---|---|
| `100` | Missing or wrong API key. |
| `200` | Missing `t` parameter. |
| `202` | Unknown function. |
| `203` | Newznab function Nebula doesn't offer (`details`, `getnfo`, `register`, cart and comments). |
| `300` | `t=get` with an unknown id, or the indexer couldn't deliver the `.nzb`. |
| `900` | Every indexer failed for that search. |

## Differences with NZBHydra2

Nebula covers what the *arr apps and Prowlarr use from NZBHydra2 (Newznab search and NZB proxying, one API key, category list, duplicate grouping and per-indexer statistics). It doesn't include:

- **Torznab** (torrent indexers): Nebula only manages Newznab (Usenet) indexers, so there is nothing to expose there.
- Query generation (turning an IMDb/TVDB id into a title for indexers that don't support ids), result caching between pages, a web interface or multiple users.
- JSON output (`o=json`): responses are always XML.
