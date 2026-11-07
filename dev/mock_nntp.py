"""
Servidor NNTP simulado para probar Nebula sin un proveedor real.

Publica (en memoria) todos los archivos de una carpeta como artículos yEnc,
genera el .nzb correspondiente y lo sirve también por HTTP.

Uso:
  python dev/mock_nntp.py --src CARPETA --nzb salida.nzb [--port 1119] [--http 8119]
                          [--user test --password test] [--missing 3] [--kbps 800]
                          [--max-conns 20] [--nzb-password clave]
                          [--debrid-key debridkey] [--debrid-secs 8] [--debrid-kbps 0] [--debrid-fail]

También simula las API de TorBox (/torbox/v1/api) y Premiumize (/premiumize/api): reciben el NZB,
«descargan» durante --debrid-secs segundos y sirven los archivos de --src con soporte de rangos.
En Nebula: NEBULA_TORBOX_URL=http://127.0.0.1:8119/torbox/v1/api
           NEBULA_PREMIUMIZE_URL=http://127.0.0.1:8119/premiumize/api  (API key: --debrid-key)
"""
import argparse
import asyncio
import hashlib
import itertools
import json
import os
import random
import re
import threading
import time
import zlib
from email.parser import BytesParser
from email.policy import default as email_policy
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, quote, unquote, urlparse
from xml.sax.saxutils import escape, quoteattr

PART = 500 * 1024
LINE = 128
TABLE = bytes(((i + 42) % 256) for i in range(256))
CRIT = re.compile(rb"[\x00\x0a\x0d=]")


def yenc_lines(data: bytes):
    enc = data.translate(TABLE)
    enc = CRIT.sub(lambda m: b"=" + bytes([(m.group()[0] + 64) % 256]), enc)
    lines, pos, n = [], 0, len(enc)
    while pos < n:
        end = min(pos + LINE, n)
        if enc[end - 1] == 0x3D and end < n:  # no partir una secuencia de escape
            end += 1
        lines.append(enc[pos:end])
        pos = end
    return lines


def build_articles(src):
    store, files = {}, []
    for name in sorted(os.listdir(src)):
        path = os.path.join(src, name)
        if not os.path.isfile(path):
            continue
        data = open(path, "rb").read()
        total = max(1, (len(data) + PART - 1) // PART)
        segs = []
        for i in range(total):
            chunk = data[i * PART:(i + 1) * PART]
            begin = i * PART + 1
            head = f"=ybegin part={i + 1} total={total} line={LINE} size={len(data)} name={name}".encode()
            ypart = f"=ypart begin={begin} end={begin + len(chunk) - 1}".encode()
            yend = f"=yend size={len(chunk)} part={i + 1} pcrc32={zlib.crc32(chunk) & 0xffffffff:08x}".encode()
            lines = [head, ypart] + yenc_lines(chunk) + [yend]
            body = b"\r\n".join((b"." + l if l.startswith(b".") else l) for l in lines) + b"\r\n.\r\n"
            mid = f"{name.replace(' ', '_')}.{i + 1}.{random.randrange(10**9)}@mock.nebula"
            store[mid] = body
            segs.append((i + 1, len(body), mid))
        files.append((name, total, segs))
        print(f"  {name}: {len(data)} bytes, {total} artículos")
    return store, files


def write_nzb(files, out, password=None, title=None):
    now = int(time.time()) - 3600
    parts = ['<?xml version="1.0" encoding="UTF-8"?>',
             '<!DOCTYPE nzb PUBLIC "-//newzBin//DTD NZB 1.1//EN" "http://www.newzbin.com/DTD/nzb/nzb-1.1.dtd">',
             '<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">', " <head>"]
    if title:
        parts.append(f'  <meta type="title">{escape(title)}</meta>')
    if password:
        parts.append(f'  <meta type="password">{escape(password)}</meta>')
    parts.append(" </head>")
    for idx, (name, total, segs) in enumerate(files):
        subject = f'[{idx + 1}/{len(files)}] - "{name}" yEnc (1/{total})'
        parts.append(f' <file poster="mock@nebula" date="{now}" subject={quoteattr(subject)}>')
        parts.append("  <groups><group>alt.binaries.test</group></groups>")
        parts.append("  <segments>")
        for num, size, mid in segs:
            parts.append(f'   <segment bytes="{size}" number="{num}">{escape(mid)}</segment>')
        parts.append("  </segments>")
        parts.append(" </file>")
    parts.append("</nzb>")
    with open(out, "w", encoding="utf-8") as f:
        f.write("\n".join(parts))


def multipart_fields(ctype, body):
    """Campos de un formulario multipart: {nombre: (nombre_archivo, bytes)}."""
    msg = BytesParser(policy=email_policy).parsebytes(b"Content-Type: " + ctype.encode() + b"\r\n\r\n" + body)
    out = {}
    for part in msg.iter_parts():
        name = part.get_param("name", header="content-disposition")
        out[name] = (part.get_filename(), part.get_payload(decode=True) or b"")
    return out


class MockDebrid:
    """Estado compartido de las API debrid simuladas (TorBox y Premiumize)."""

    def __init__(self, src, key, secs, kbps, fail):
        self.src, self.key, self.secs, self.kbps, self.fail = src, key, secs, kbps, fail
        self.items = {}
        self.ids = itertools.count(1)
        self.lock = threading.Lock()
        self.files = []
        for root, _, names in os.walk(src):
            for n in sorted(names):
                full = os.path.join(root, n)
                self.files.append((os.path.relpath(full, src).replace(os.sep, "/"), full, os.path.getsize(full)))

    def add(self, name, nzb):
        with self.lock:
            h = hashlib.md5(nzb).hexdigest()
            for it in self.items.values():
                if it["hash"] == h:
                    return it, True
            it = {"id": next(self.ids), "name": name, "hash": h, "start": time.time()}
            self.items[it["id"]] = it
            return it, False

    def progress(self, it):
        return 1.0 if self.secs <= 0 else min(1.0, (time.time() - it["start"]) / self.secs)

    def file(self, index):
        return self.files[index] if 0 <= index < len(self.files) else None


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True)
    ap.add_argument("--nzb", required=True)
    ap.add_argument("--port", type=int, default=1119)
    ap.add_argument("--http", type=int, default=8119)
    ap.add_argument("--user", default="test")
    ap.add_argument("--password", default="test")
    ap.add_argument("--missing", type=int, default=0, help="artículos de datos que devolverán 430")
    ap.add_argument("--kbps", type=int, default=0, help="límite por conexión en KB/s (0 = sin límite)")
    ap.add_argument("--max-conns", type=int, default=50)
    ap.add_argument("--nzb-password", default=None)
    ap.add_argument("--title", default=None)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--indexer-key", default="testkey", help="API key de la API Newznab simulada (/api)")
    ap.add_argument("--indexer-title", default=None, help="título del release en la API Newznab")
    ap.add_argument("--debrid-key", default="debridkey", help="API key de TorBox/Premiumize simulados")
    ap.add_argument("--debrid-secs", type=float, default=8, help="segundos que tarda la «descarga» remota")
    ap.add_argument("--debrid-kbps", type=int, default=0, help="límite por conexión al bajar archivos (KB/s)")
    ap.add_argument("--debrid-fail", action="store_true", help="la descarga remota termina con error")
    args = ap.parse_args()
    random.seed(args.seed)

    print("Generando artículos…")
    store, files = build_articles(args.src)
    write_nzb(files, args.nzb, args.nzb_password, args.title)
    data_ids = [mid for (name, _, segs) in files if not name.lower().endswith(".par2") for (_, _, mid) in segs]
    missing = set(random.sample(data_ids, min(args.missing, len(data_ids))))
    counts = {"body": 0, "stat": 0}
    print(f"NZB: {args.nzb} ({len(store)} artículos, {len(missing)} ausentes)")

    # HTTP para probar el flujo "pegar enlace"
    nzb_dir = os.path.dirname(os.path.abspath(args.nzb))
    debrid = MockDebrid(args.src, args.debrid_key, args.debrid_secs, args.debrid_kbps, args.debrid_fail)
    http_base = f"http://127.0.0.1:{args.http}"

    class H(SimpleHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def __init__(self, *a, **k):
            super().__init__(*a, directory=nzb_dir, **k)

        def log_message(self, *a):
            pass

        def do_POST(self):
            u = urlparse(self.path)
            q = {k: v[0] for k, v in parse_qs(u.query).items()}
            body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
            if u.path.startswith("/torbox/"):
                return self._torbox("POST", u.path, q, body)
            if u.path.startswith("/premiumize/"):
                return self._premiumize("POST", u.path, q, body)
            return self._send("no encontrado", "text/plain", status=404)

        # API Newznab mínima: t=caps, t=search y t=get
        def do_GET(self):
            u = urlparse(self.path)
            q = {k: v[0] for k, v in parse_qs(u.query).items()}
            if u.path == "/_stats":
                return self._send(json.dumps(counts), "application/json")
            if u.path.startswith("/torbox/dl/") or u.path.startswith("/premiumize/dl/"):
                return self._serve_file(u.path)
            if u.path.startswith("/torbox/"):
                return self._torbox("GET", u.path, q, b"")
            if u.path.startswith("/premiumize/"):
                return self._premiumize("GET", u.path, q, b"")
            if not u.path.rstrip("/").endswith("/api"):
                return super().do_GET()
            t = q.get("t", "")
            base = f"http://127.0.0.1:{args.http}/api"
            title = args.indexer_title or os.path.splitext(os.path.basename(args.nzb))[0]
            if t == "caps":
                body = ('<?xml version="1.0" encoding="UTF-8"?><caps><server title="Mock Newznab"/>'
                        '<limits max="100" default="50"/><searching><search available="yes"/></searching></caps>')
                return self._send(body, "application/xml")
            if q.get("apikey") != args.indexer_key:
                return self._send('<?xml version="1.0" encoding="UTF-8"?><error code="100" description="Incorrect user credentials"/>', "application/xml")
            if t == "get":
                data = open(args.nzb, "rb").read()
                return self._send(data, "application/x-nzb", {"Content-Disposition": f'attachment; filename="{title}.nzb"'})
            if t == "search":
                words = [w for w in q.get("q", "").lower().split() if w]
                match = all(w in title.lower() for w in words)
                items = ""
                if match and q.get("offset", "0") == "0":
                    size = sum(os.path.getsize(os.path.join(args.src, f)) for f in os.listdir(args.src))
                    date = time.strftime("%a, %d %b %Y %H:%M:%S +0000", time.gmtime(time.time() - 7200))
                    link = escape(f"{base}?t=get&id=1&apikey={args.indexer_key}")
                    items = (f"<item><title>{escape(title)}</title><guid isPermaLink=\"true\">{base}/details/1</guid>"
                             f"<link>{link}</link><pubDate>{date}</pubDate><category>Movies &gt; HD</category>"
                             f'<enclosure url="{link}" length="{size}" type="application/x-nzb"/>'
                             f'<newznab:attr name="category" value="2000"/><newznab:attr name="category" value="2040"/>'
                             f'<newznab:attr name="size" value="{size}"/><newznab:attr name="grabs" value="7"/>'
                             f'<newznab:attr name="usenetdate" value="{date}"/></item>')
                total = 1 if items else 0
                body = ('<?xml version="1.0" encoding="UTF-8"?><rss version="2.0" '
                        'xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/"><channel>'
                        f'<newznab:response offset="0" total="{total}"/>{items}</channel></rss>')
                return self._send(body, "application/rss+xml")
            return self._send('<?xml version="1.0"?><error code="202" description="No such function"/>', "application/xml")

        # ---- TorBox simulado: {"success", "error", "detail", "data"} ----
        def _tb(self, data=None, error=None, detail="", status=200):
            body = {"success": error is None, "error": error, "detail": detail, "data": data}
            return self._send(json.dumps(body), "application/json", status=status)

        def _tb_item(self, it):
            p = debrid.progress(it)
            done = p >= 1.0
            state = ("failed" if debrid.fail else "completed") if done else "downloading"
            files = [{"id": i, "name": f"{it['name']}/{rel}", "short_name": os.path.basename(rel), "size": size,
                      "mimetype": "application/octet-stream"} for i, (rel, _, size) in enumerate(debrid.files)]
            total = sum(f[2] for f in debrid.files)
            return {"id": it["id"], "hash": it["hash"], "name": it["name"], "size": total, "download_state": state,
                    "progress": p, "download_speed": 0 if done else int(total / max(debrid.secs, 1)),
                    "eta": int(max(0, debrid.secs * (1 - p))), "download_finished": done and not debrid.fail,
                    "download_present": done and not debrid.fail, "files": files if done else []}

        def _torbox(self, method, path, q, body):
            ep = path.split("/v1/api/", 1)[-1].strip("/")
            if ep == "usenet/requestdl":
                auth_ok = q.get("token") == debrid.key
            else:
                auth_ok = self.headers.get("Authorization", "") == f"Bearer {debrid.key}"
            if not auth_ok:
                return self._tb(error="BAD_TOKEN", detail="Invalid API token.", status=403)
            if ep == "user/me":
                return self._tb({"email": "mock@nebula.local", "plan": 2, "premium_expires_at": "2027-01-01T00:00:00Z"})
            if ep == "usenet/createusenetdownload" and method == "POST":
                fields = multipart_fields(self.headers.get("Content-Type", ""), body)
                nzb = fields.get("file", (None, b""))[1]
                if b"<nzb" not in nzb[:4096].lower():
                    return self._tb(error="BOZO_NZB", detail="Invalid NZB.", status=400)
                name = fields.get("name", (None, b"descarga"))[1].decode() or "descarga"
                it, cached = debrid.add(name, nzb)
                print(f"[TorBox] NZB recibido: {name} (id {it['id']}, post_processing={fields.get('post_processing', (None, b''))[1].decode()})")
                return self._tb({"usenetdownload_id": it["id"], "hash": it["hash"], "auth_id": "mock"},
                                detail="Found cached usenet download." if cached else "Usenet download added.")
            if ep == "usenet/mylist":
                if "id" in q:
                    it = debrid.items.get(int(q["id"]))
                    if not it:
                        return self._tb(error="ITEM_NOT_FOUND", detail="Download not found.", status=404)
                    return self._tb(self._tb_item(it))
                return self._tb([self._tb_item(it) for it in debrid.items.values()])
            if ep == "usenet/requestdl":
                it = debrid.items.get(int(q.get("usenet_id", "0")))
                f = debrid.file(int(q.get("file_id", "-1")))
                if not it or not f:
                    return self._tb(error="ITEM_NOT_FOUND", detail="File not found.", status=404)
                return self._tb(f"{http_base}/torbox/dl/{it['id']}/{q['file_id']}/{quote(os.path.basename(f[0]))}")
            if ep == "usenet/controlusenetdownload" and method == "POST":
                req = json.loads(body or b"{}")
                debrid.items.pop(int(req.get("usenet_id", 0)), None)
                print(f"[TorBox] {req.get('operation')} {req.get('usenet_id')}")
                return self._tb(None, detail="Deleted.")
            if ep == "usenet/checkcached":
                hashes = q.get("hash", "").split(",")
                data = [{"name": it["name"], "hash": it["hash"], "size": 0} for it in debrid.items.values() if it["hash"] in hashes]
                return self._tb(data)
            return self._tb(error="ENDPOINT_NOT_FOUND", detail=ep, status=404)

        # ---- Premiumize simulado: {"status": "success"|"error", ...} ----
        def _pm(self, data=None, error=None):
            body = {"status": "error", "message": error} if error else {"status": "success", **(data or {})}
            return self._send(json.dumps(body), "application/json")

        def _pm_content(self, tid, prefix):
            out, subdirs = [], set()
            for i, (rel, _, size) in enumerate(debrid.files):
                if not rel.startswith(prefix):
                    continue
                rest = rel[len(prefix):]
                if "/" in rest:
                    subdirs.add(rest.split("/", 1)[0])
                    continue
                out.append({"id": f"i-{tid}-{i}", "name": rest, "type": "file", "size": size,
                            "link": f"{http_base}/premiumize/dl/{tid}/{i}/{quote(rest)}"})
            for d in sorted(subdirs):
                out.append({"id": f"f-{tid}-{prefix}{d}/", "name": d, "type": "folder"})
            return out

        def _premiumize(self, method, path, q, body):
            ep = path.split("/premiumize/api/", 1)[-1].strip("/")
            if q.get("apikey") != debrid.key:
                return self._pm(error="Not logged in.")
            form = {k: v[0] for k, v in parse_qs(body.decode(errors="replace")).items()} if method == "POST" and not ep.endswith("create") else {}
            if ep == "account/info":
                return self._pm({"customer_id": 1, "premium_until": int(time.time()) + 30 * 86400, "limit_used": 0.1, "space_used": 0})
            if ep == "transfer/create":
                fields = multipart_fields(self.headers.get("Content-Type", ""), body)
                fname, nzb = fields.get("src", (None, b""))
                if b"<nzb" not in nzb[:4096].lower():
                    return self._pm(error="Invalid file.")
                it, _ = debrid.add((fname or "descarga.nzb").removesuffix(".nzb"), nzb)
                print(f"[Premiumize] NZB recibido: {fname} (id {it['id']})")
                return self._pm({"id": f"t{it['id']}", "name": fname, "type": "nzb"})
            if ep == "transfer/list":
                transfers = []
                for it in debrid.items.values():
                    p = debrid.progress(it)
                    done = p >= 1.0
                    status = ("error" if debrid.fail else "finished") if done else "running"
                    msg = "Not enough repair blocks." if debrid.fail and done else ("" if done else f"Downloading at 42.0 MB/s. {int(p * 100)}% finished.")
                    transfers.append({"id": f"t{it['id']}", "name": it["name"] + ".nzb", "status": status, "progress": p,
                                      "message": msg, "folder_id": f"f-{it['id']}-" if done and not debrid.fail else None,
                                      "file_id": None})
                return self._pm({"transfers": transfers})
            if ep == "folder/list":
                fid = q.get("id", "")
                tid, _, prefix = fid[2:].partition("-")
                return self._pm({"content": self._pm_content(tid, prefix), "name": "carpeta"})
            if ep == "item/details":
                _, tid, idx = q.get("id", "i-0-0").split("-", 2)
                f = debrid.file(int(idx))
                if not f:
                    return self._pm(error="Item not found.")
                return self._pm({"id": q["id"], "name": os.path.basename(f[0]), "size": f[2],
                                 "link": f"{http_base}/premiumize/dl/{tid}/{idx}/{quote(os.path.basename(f[0]))}"})
            if ep in ("folder/delete", "item/delete", "transfer/delete"):
                print(f"[Premiumize] {ep} {form.get('id')}")
                if ep == "transfer/delete":
                    debrid.items.pop(int(form.get("id", "t0")[1:] or 0), None)
                return self._pm({})
            return self._pm(error=f"Unknown endpoint {ep}")

        def _serve_file(self, path):
            parts = path.split("/")
            f = debrid.file(int(parts[4])) if len(parts) > 4 and parts[4].isdigit() else None
            if not f:
                return self._send("no encontrado", "text/plain", status=404)
            _, full, size = f
            start, end, status = 0, size - 1, 200
            rng = self.headers.get("Range", "")
            m = re.match(r"bytes=(\d+)-(\d*)$", rng)
            if m:
                start = int(m.group(1))
                end = min(int(m.group(2)) if m.group(2) else size - 1, size - 1)
                if start >= size or start > end:
                    return self._send("", "text/plain", status=416, headers={"Content-Range": f"bytes */{size}"})
                status = 206
            self.send_response(status)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Length", str(end - start + 1))
            if status == 206:
                self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
            self.end_headers()
            step = 256 * 1024
            with open(full, "rb") as fh:
                fh.seek(start)
                left = end - start + 1
                try:
                    while left > 0:
                        chunk = fh.read(min(step, left))
                        if not chunk:
                            break
                        self.wfile.write(chunk)
                        left -= len(chunk)
                        if debrid.kbps:
                            time.sleep(len(chunk) / (debrid.kbps * 1024))
                except (ConnectionResetError, BrokenPipeError):
                    pass

        def _send(self, body, ctype, headers=None, status=200):
            data = body.encode() if isinstance(body, str) else body
            self.send_response(status)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(data)))
            for k, v in (headers or {}).items():
                self.send_header(k, v)
            self.end_headers()
            self.wfile.write(data)

    httpd = ThreadingHTTPServer(("127.0.0.1", args.http), H)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    print(f"HTTP: http://127.0.0.1:{args.http}/{os.path.basename(args.nzb)}")
    print(f"TorBox simulado: {http_base}/torbox/v1/api · Premiumize simulado: {http_base}/premiumize/api (API key {args.debrid_key})")

    active = 0

    async def handle(reader, writer):
        nonlocal active
        if active >= args.max_conns:
            writer.write(b"502 Too many connections from your IP\r\n")
            await writer.drain()
            writer.close()
            return
        active += 1
        authed = False
        user = None
        try:
            writer.write(b"200 Mock NNTP listo\r\n")
            await writer.drain()
            while True:
                line = await reader.readline()
                if not line:
                    break
                cmd = line.decode("latin-1").strip()
                up = cmd.upper()
                if up.startswith("AUTHINFO USER"):
                    user = cmd[14:].strip()
                    writer.write(b"381 Password required\r\n")
                elif up.startswith("AUTHINFO PASS"):
                    pw = cmd[14:].strip()
                    if user == args.user and pw == args.password:
                        authed = True
                        writer.write(b"281 Authentication accepted\r\n")
                    else:
                        writer.write(b"481 Authentication failed\r\n")
                elif up.startswith("STAT"):
                    counts["stat"] += 1
                    if not authed:
                        writer.write(b"480 Authentication required\r\n")
                    else:
                        mid = cmd[4:].strip().strip("<>")
                        if mid in store and mid not in missing:
                            writer.write(f"223 0 <{mid}>\r\n".encode())
                        else:
                            writer.write(b"430 No such article\r\n")
                elif up.startswith("BODY"):
                    counts["body"] += 1
                    if not authed:
                        writer.write(b"480 Authentication required\r\n")
                    else:
                        mid = cmd[4:].strip().strip("<>")
                        body = store.get(mid)
                        if body is None or mid in missing:
                            writer.write(b"430 No such article\r\n")
                        else:
                            writer.write(f"222 0 <{mid}>\r\n".encode())
                            if args.kbps:
                                step = 32 * 1024
                                for i in range(0, len(body), step):
                                    writer.write(body[i:i + step])
                                    await writer.drain()
                                    await asyncio.sleep(step / (args.kbps * 1024))
                            else:
                                writer.write(body)
                elif up.startswith("DATE"):
                    writer.write(time.strftime("111 %Y%m%d%H%M%S\r\n", time.gmtime()).encode())
                elif up.startswith("QUIT"):
                    writer.write(b"205 Bye\r\n")
                    await writer.drain()
                    break
                else:
                    writer.write(b"500 Unknown command\r\n")
                await writer.drain()
        except (ConnectionResetError, BrokenPipeError):
            pass
        finally:
            active -= 1
            writer.close()

    server = await asyncio.start_server(handle, "127.0.0.1", args.port)
    print(f"NNTP: 127.0.0.1:{args.port} (usuario {args.user} / {args.password})")
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
