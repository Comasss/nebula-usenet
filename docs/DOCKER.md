# Nebula in Docker (NAS, Unraid, Synology)

The `ghcr.io/comasss/nebula` image runs [nebula-server](SERVER.md): the interface in the browser and the Newznab and SABnzbd APIs on port `5080`. It's built for `amd64` and `arm64` (a Raspberry Pi 4/5 or an ARM NAS), comes with **par2cmdline-turbo** and **7-Zip** (with RAR), and follows the [linuxserver.io](https://docs.linuxserver.io/general/understanding-puid-and-pgid/) conventions: `PUID`, `PGID`, `UMASK` and `TZ`, settings in `/config` and downloads in `/downloads`.

| Tag | What it is |
|---|---|
| `latest` | The latest published version. |
| `vX.Y.Z` | A specific version (`v1.10.1`…), the same as the [Release](../../../releases) with that number. |

## Quick start

```bash
docker run -d --name nebula \
  -p 5080:5080 \
  -e PUID=1000 -e PGID=1000 -e TZ=Europe/Madrid \
  -v /path/to/nebula:/config \
  -v /path/to/downloads:/downloads \
  --restart unless-stopped \
  ghcr.io/comasss/nebula:latest
```

Open `http://<server>:5080`, create your user and add your Usenet servers. Downloads go to `/downloads` (that is, `/path/to/downloads` on the host).

## With Sonarr and Radarr (Compose)

[`deploy/docker-compose.yml`](../deploy/docker-compose.yml) starts Nebula, Sonarr and Radarr with the same download folder mounted at `/downloads` in all three. The path Nebula reports to Sonarr then exists inside Sonarr's container too, so no **Remote Path Mapping** is needed.

```bash
mkdir nebula-stack && cd nebula-stack
curl -O https://raw.githubusercontent.com/Comasss/nebula-usenet/main/deploy/docker-compose.yml
# edit PUID, PGID, TZ and the folders
docker compose up -d
```

Then:

1. In Nebula (`http://<server>:5080`): **Settings → API**, turn on **Enable API** and copy the key. In **Settings → Categories**, create `tv` and `movies`.
2. In Sonarr (`:8989`) and Radarr (`:7878`): **Settings → Download Clients → + → SABnzbd** with host `nebula` (the service name; or the server's IP), port `5080`, the API key and the category (`tv` or `movies`). Press **Test** and **Save**.

More about the SABnzbd API in [SABNZBD.md](SABNZBD.md).

## Settings

| Variable | Default | What it does |
|---|---|---|
| `PUID`, `PGID` | `1000` | User and group Nebula runs as. Use the ones that own your folders (`id <user>` on the host). |
| `UMASK` | `022` | Permissions of what Nebula creates. `002` lets the group write too (handy if Sonarr runs as another user in the same group). |
| `TZ` | `Etc/UTC` | Time zone (`Europe/Madrid`, `America/New_York`…). |
| `NEBULA_PORT` | `5080` | Port inside the container. Usually you change only the host side of `-p`. |
| `NEBULA_DOWNLOAD_DIR` | `/downloads` | Default download folder, used until you choose another one in **Settings → Folders**. |

| Volume | What it holds |
|---|---|
| `/config` | `config.json`, the queue, the history, statistics, logs, `auth.json` (the web user) and `secret.key` (the key that encrypts passwords in `config.json`). Back it up as a whole. |
| `/downloads` | Downloads. On first start only the folder itself is given to `PUID:PGID`, never its contents. |

On start, `/config` is given to `PUID:PGID` if it belongs to someone else. Nebula then drops root and runs as that user. Running the container with `--user` also works; then `PUID`/`PGID` are ignored and the folders must already be writable by that user.

## Health and stopping

The image has a `HEALTHCHECK` (`nebula-server healthcheck`, which asks `/health`), so `docker ps` shows `healthy` and Compose can wait for it. `docker stop` sends SIGTERM: Nebula saves the queue and the history and exits in a moment, and downloads continue on the next start.

## Updating

```bash
docker compose pull nebula && docker compose up -d nebula
# or: docker pull ghcr.io/comasss/nebula:latest, then recreate the container
```

Your settings, queue and history stay in `/config`. To stay on a version, use its tag (`:v1.10.1`) instead of `latest`.

## Unraid

**Docker → Add Container**, with **Advanced View** on:

| Field | Value |
|---|---|
| Name | `nebula` |
| Repository | `ghcr.io/comasss/nebula:latest` |
| WebUI | `http://[IP]:[PORT:5080]/` |
| Network Type | `bridge` |
| Port | container `5080` → host `5080` |
| Path | container `/config` → host `/mnt/user/appdata/nebula` |
| Path | container `/downloads` → host `/mnt/user/downloads` (the same share Sonarr and Radarr see as `/downloads`) |
| Variable | `PUID` = `99`, `PGID` = `100` (Unraid's `nobody:users`), `UMASK` = `002`, `TZ` = your time zone |

## Synology (DSM 7, Container Manager)

1. In **File Station**, create `docker/nebula` and a downloads folder (for example `downloads` in a shared folder).
2. Find your user's ids: in **Control Panel → Terminal & SNMP** enable SSH, connect and run `id`. The usual values are `uid=1026` and `gid=100` (`users`).
3. **Container Manager → Registry → Settings → Add**: `https://ghcr.io` (or, in **Project**, use the Compose file directly). Download `ghcr.io/comasss/nebula` with the tag `latest`.
4. **Container → Create**:
   - Port: local `5080` → container `5080`.
   - Volumes: `docker/nebula` → `/config`, your downloads folder → `/downloads`.
   - Environment: `PUID=1026`, `PGID=100`, `UMASK=002`, `TZ=Europe/Madrid`.
   - Enable auto-restart.
5. Open `http://<nas>:5080`.

With Sonarr and Radarr on the same NAS, the simplest way is **Container Manager → Project → Create** with [`deploy/docker-compose.yml`](../deploy/docker-compose.yml), changing the folders to `/volume1/docker/…` and the ids to yours.

## Without Docker

The [Release](../../../releases) also has `nebula-server_<version>_linux_x64` and `_linux_arm64`: one static binary that runs on any Linux distribution. [`deploy/nebula-server.service`](../deploy/nebula-server.service) is a systemd unit for it, with the steps to install it at the top. See [SERVER.md](SERVER.md); in that case par2 and 7-Zip come from the system (`apt install par2 7zip`).

## Building the image

```bash
docker build -t nebula .
```

The [`Dockerfile`](../Dockerfile) builds everything in stages: the interface with Node, `nebula-server` as a static binary (musl, with OpenSSL inside), and par2cmdline-turbo and 7-Zip from source (`PAR2_VERSION` and `SEVENZIP_VERSION` are build arguments). The final image is `debian:trixie-slim` with those three programs. To get only the binary:

```bash
docker buildx build --target server-bin --output type=local,dest=out .
```

CI (`.github/workflows/build.yml`) builds it on an amd64 runner and on an arm64 one. It checks that the image starts healthy, keeps `PUID` and stops cleanly. On `main` and on `v*` tags it publishes `latest` and `vX.Y.Z` to GHCR.
