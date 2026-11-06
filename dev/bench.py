#!/usr/bin/env python3
"""
Compara la velocidad de descarga de Nebula con SABnzbd y NZBGet (si están instalados).

Crea un caso realista (un vídeo aleatorio en 7z por volúmenes con un 5 % de PAR2), lo sirve con
el mock NNTP rápido (dev/nntp-bench) quitando unos pocos artículos para que haya que reparar, y
lo descarga con cada programa, de uno en uno y con las mismas conexiones. Mide:

  - velocidad media y pico (por el lado del servidor, lo que el cliente recibe),
  - CPU (segundos y % medio de un núcleo) y RAM máxima de todo el árbol de procesos,
  - tiempo de descarga y tiempo total con verificación, reparación y extracción,
  - y comprueba que el archivo extraído es idéntico al original.

Uso (desde la raíz del repositorio, en Linux):
  python3 dev/bench.py [--size-mb 1000] [--conns 20] [--missing 6] [--clients nebula,sabnzbd,nzbget]
                       [--work /tmp/nebula-bench] [--runs 1] [--json resultados.json] [--keep]

Necesita par2 y 7z, y compila `nebula-cli` y `nntp-bench` en release si no están. SABnzbd
(`sabnzbdplus`) y NZBGet (`nzbget`) se usan si están en el PATH; si no, se saltan.
"""
import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TICK = os.sysconf("SC_CLK_TCK")
PAGE = os.sysconf("SC_PAGE_SIZE")
PORT = 1219
TITLE = "Bench.Movie.2026.1080p"


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def run(cmd, **kw):
    return subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kw)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def md5(path):
    h = hashlib.md5()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# Medición del árbol de procesos con /proc
# ---------------------------------------------------------------------------

def children_map():
    kids = {}
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            with open(f"/proc/{p}/stat") as f:
                ppid = int(f.read().rsplit(")", 1)[1].split()[1])
            kids.setdefault(ppid, []).append(int(p))
        except (OSError, IndexError, ValueError):
            pass
    return kids


def tree(pid):
    kids, out, todo = children_map(), [], [pid]
    while todo:
        p = todo.pop()
        out.append(p)
        todo.extend(kids.get(p, []))
    return out


def usage(pid):
    """(segundos de CPU, bytes de RAM) del proceso y sus hijos vivos."""
    cpu = rss = 0
    for p in tree(pid):
        try:
            with open(f"/proc/{p}/stat") as f:
                fields = f.read().rsplit(")", 1)[1].split()
            # utime, stime, cutime, cstime (los hijos que ya terminaron, p. ej. par2 y 7z)
            cpu += sum(int(x) for x in fields[11:15])
            rss += int(fields[21]) * PAGE
        except (OSError, IndexError, ValueError):
            pass
    return cpu / TICK, rss


class Sampler(threading.Thread):
    """Muestrea CPU y RAM del árbol de un proceso cada 100 ms."""

    def __init__(self, pid):
        super().__init__(daemon=True)
        self.pid, self.stop_ev = pid, threading.Event()
        self.cpu0, _ = usage(pid)
        self.cpu, self.peak_rss = self.cpu0, 0

    def run(self):
        while not self.stop_ev.is_set():
            cpu, rss = usage(self.pid)
            self.cpu = max(self.cpu, cpu)
            self.peak_rss = max(self.peak_rss, rss)
            time.sleep(0.1)

    def stop(self):
        self.stop_ev.set()
        self.join()
        return self.cpu - self.cpu0, self.peak_rss


# ---------------------------------------------------------------------------
# El servidor simulado
# ---------------------------------------------------------------------------

class Mock:
    def __init__(self, binary, src, nzb, conns, missing):
        self.samples = []  # (t, bytes enviados)
        self.lock = threading.Lock()
        self.p = subprocess.Popen([binary, "--src", src, "--nzb", nzb, "--port", str(PORT), "--max-conns", str(conns + 10), "--missing", str(missing)],
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        for line in self.p.stdout:
            if line.startswith("READY"):
                self.t0 = time.monotonic()
                break
        else:
            sys.exit(f"nntp-bench no arrancó (¿el puerto {PORT} está ocupado?)")
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.p.stdout:
            if line.startswith("SENT "):
                ms, sent = line.split()[1:3]
                with self.lock:
                    self.samples.append((self.t0 + int(ms) / 1000, int(sent)))

    def mark(self):
        with self.lock:
            return self.samples[-1][1] if self.samples else 0

    def window(self, t0, base):
        """Lo enviado desde `base`: (bytes, primer byte, último byte, pico en MB/s, segundos con datos llegando)."""
        with self.lock:
            s = [(t, b - base) for t, b in self.samples if b > base]
        if not s:
            return 0, t0, t0, 0, 0
        first = s[0][0] - 0.1  # la primera muestra llega como mucho 100 ms después
        s.insert(0, (first, 0))
        # Pico: el mejor tramo de al menos medio segundo (las descargas en local duran poco).
        peak, j = 0.0, 0
        for i in range(1, len(s)):
            while j + 1 < i and s[i][0] - s[j + 1][0] >= 0.5:
                j += 1
            span = s[i][0] - s[j][0]
            if span >= 0.5 or (j == 0 and i == len(s) - 1):
                peak = max(peak, (s[i][1] - s[j][1]) / span)
        # Tiempo con datos llegando: sin las pausas de más de medio segundo (p. ej. la verificación
        # antes de pedir los bloques PAR2 que faltan).
        active = sum(min(b[0] - a[0], 0.5) for a, b in zip(s, s[1:]))
        return s[-1][1], first, s[-1][0], peak / 1e6, active

    def close(self):
        self.p.send_signal(signal.SIGINT)
        self.p.wait(timeout=10)


# ---------------------------------------------------------------------------
# Los clientes
# ---------------------------------------------------------------------------

def http_json(url, data=None, timeout=10, headers=None):
    req = urllib.request.Request(url, data=data, headers=headers or {})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def kill_tree(p):
    """Mata el proceso y todo lo que haya lanzado (par2, 7z…)."""
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except OSError:
        pass
    p.wait()


def wait_until(fn, timeout, step=0.2):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            v = fn()
            if v:
                return v
        except Exception:
            pass
        time.sleep(step)
    raise TimeoutError


def find_file(root, size):
    for d, _, files in os.walk(root):
        for f in files:
            p = os.path.join(d, f)
            if f.endswith(".mkv") and os.path.getsize(p) == size:
                return p
    return None


class Nebula:
    name = "Nebula"

    def __init__(self, args):
        self.bin = os.path.join(ROOT, "target", "release", "nebula-cli")

    def version(self):
        return subprocess.run([self.bin, "--version"], capture_output=True, text=True).stdout.split()[-1]

    def start(self, work, nzb, conns):
        out = os.path.join(work, "out")
        os.makedirs(out)
        env = dict(os.environ, HOME=work, XDG_CONFIG_HOME=os.path.join(work, "cfg"), XDG_DATA_HOME=os.path.join(work, "data"))
        self.p = subprocess.Popen([self.bin, "get", nzb, "-o", out, "-s", f"nntp://test:test@127.0.0.1:{PORT}?conns={conns}"],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, env=env)
        return self.p.pid, out

    def wait(self, timeout):
        rc = self.p.wait(timeout=timeout)
        if rc != 0:
            raise RuntimeError(f"nebula get salió con {rc}: {self.p.stderr.read()[-400:]}")

    def stop(self):
        if self.p.poll() is None:
            self.p.kill()


class Nzbget:
    name = "NZBGet"
    bin = shutil.which("nzbget")

    def __init__(self, args):
        self.port = free_port()

    def version(self):
        out = subprocess.run([self.bin, "-v"], capture_output=True, text=True).stdout
        m = re.search(r"(\d+\.\d+[\w.-]*)", out)
        return m.group(1) if m else "?"

    def rpc(self, method, params=()):
        body = json.dumps({"method": method, "params": list(params)}).encode()
        auth = "Basic " + base64.b64encode(b"nzbget:bench").decode()
        return http_json(f"http://127.0.0.1:{self.port}/jsonrpc", body, headers={"Authorization": auth})["result"]

    def start(self, work, nzb, conns):
        conf = os.path.join(work, "nzbget.conf")
        # NZBGet 21 (el de Ubuntu 24.04) se cae con "buffer overflow detected" en el post-procesado
        # (tras reparar): con esa versión solo se mide la descarga y la tabla lo dice.
        self.unpack = not self.version().startswith("21.")
        main = os.path.join(work, "nzbget")
        with open(conf, "w") as f:
            f.write("\n".join([
                f"MainDir={main}", "DestDir=${MainDir}/complete", "InterDir=${MainDir}/intermediate", "NzbDir=${MainDir}/nzb",
                "QueueDir=${MainDir}/queue", "TempDir=${MainDir}/tmp", "LockFile=${MainDir}/nzbget.lock", "LogFile=${MainDir}/nzbget.log",
                "ScriptDir=${MainDir}/scripts", "WebDir=/usr/share/nzbget/webui", "ConfigTemplate=/usr/share/nzbget/nzbget.conf",
                "ControlIP=127.0.0.1", f"ControlPort={self.port}", "ControlUsername=nzbget", "ControlPassword=bench", "SecureControl=no",
                "Server1.Active=yes", "Server1.Name=bench", "Server1.Level=0", "Server1.Host=127.0.0.1", f"Server1.Port={PORT}",
                "Server1.Username=test", "Server1.Password=test", "Server1.Encryption=no", f"Server1.Connections={conns}",
                # Lo habitual en un NAS: caché de artículos, escritura directa y post-procesado completo.
                "ArticleCache=500", "DirectWrite=yes", "WriteBuffer=1024", "CrcCheck=yes", f"ParCheck={'auto' if self.unpack else 'manual'}", "ParRepair=yes",
                "ParScan=extended", f"Unpack={'yes' if self.unpack else 'no'}", "DirectUnpack=no", "UnpackCleanupDisk=yes",
                "SevenZipCmd=7z", "UnrarCmd=unrar", "HealthCheck=park", "DupeCheck=no", "WriteLog=append", "OutputMode=loggable",
                "UpdateCheck=none", "AppendCategoryDir=no", "NzbDirInterval=0",
            ]) + "\n")
        self.p = subprocess.Popen([self.bin, "-c", conf, "-s"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        wait_until(lambda: self.rpc("version"), 30)
        with open(nzb, "rb") as f:
            content = base64.b64encode(f.read()).decode()
        # append(NZBFilename, Content, Category, Priority, AddToTop, AddPaused, DupeKey, DupeScore, DupeMode, PPParameters)
        self.rpc("append", [TITLE + ".nzb", content, "", 0, False, False, "", 0, "SCORE", []])
        return self.p.pid, os.path.join(main, "complete")

    def wait(self, timeout):
        def done():
            if self.p.poll() is not None:
                return {"Status": "CRASHED (NZBGet se ha cerrado solo)"}
            h = self.rpc("history", [False])
            return h[0] if h else None
        h = wait_until(done, timeout, 0.25)
        if h["Status"].startswith("CRASHED") or (self.unpack and not h["Status"].startswith("SUCCESS")):
            raise RuntimeError(f"NZBGet: {h['Status']}")

    def stop(self):
        try:
            self.rpc("shutdown")
            self.p.wait(timeout=15)
        except Exception:
            kill_tree(self.p)


class Sabnzbd:
    name = "SABnzbd"
    bin = shutil.which("sabnzbdplus") or shutil.which("sabnzbd")

    def __init__(self, args):
        self.port = free_port()
        self.python = sab_python(self.bin)

    def version(self):
        try:
            return http_json(self.api("version"))["version"]
        except Exception:
            return "?"

    def api(self, mode, **q):
        q.update(mode=mode, apikey="benchbenchbenchbenchbenchbench12", output="json")
        return f"http://127.0.0.1:{self.port}/api?{urllib.parse.urlencode(q)}"

    def start(self, work, nzb, conns):
        home = os.path.join(work, "sab")
        os.makedirs(home)
        ini = os.path.join(home, "sabnzbd.ini")
        with open(ini, "w") as f:
            f.write("\n".join([
                "__version__ = 19", "__encoding__ = utf-8", "[misc]", "api_key = benchbenchbenchbenchbenchbench12",
                "nzb_key = benchbenchbenchbenchbenchbench34", f"download_dir = {home}/incomplete", f"complete_dir = {home}/complete",
                "host = 127.0.0.1", f"port = {self.port}", "check_new_rel = 0", "auto_browser = 0", "enable_https = 0",
                "language = en", "cache_limit = 1G", "direct_unpack = 0", "enable_unrar = 1", "enable_7zip = 1",
                "enable_par_cleanup = 1", "pre_check = 0", "no_dupes = 0", "ignore_samples = 0", "enable_all_par = 0",
                "[servers]", "[[bench]]", "name = bench", "displayname = bench", "host = 127.0.0.1", f"port = {PORT}",
                "username = test", "password = test", f"connections = {conns}", "ssl = 0", "enable = 1", "priority = 0",
            ]) + "\n")
        self.p = subprocess.Popen([self.python, "-OO", self.bin, "-f", ini, "-s", f"127.0.0.1:{self.port}", "-b", "0", "--disable-file-log"],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        wait_until(lambda: http_json(self.api("version")), 60)
        http_json(self.api("addlocalfile", name=nzb, nzbname=TITLE, pp=3))
        return self.p.pid, os.path.join(home, "complete")

    def wait(self, timeout):
        def done():
            slots = http_json(self.api("history"))["history"]["slots"]
            return slots[0] if slots and slots[0]["status"] in ("Completed", "Failed") else None
        h = wait_until(done, timeout, 0.25)
        if h["status"] != "Completed":
            raise RuntimeError(f"SABnzbd: {h.get('fail_message')}")

    def stop(self):
        try:
            http_json(self.api("shutdown"))
            self.p.wait(timeout=20)
        except Exception:
            kill_tree(self.p)


def sab_python(script):
    """Un Python que pueda cargar SABnzbd: el paquete de Ubuntu 24.04 trae sabctools para 3.12
    aunque `python3` sea otra versión."""
    first = open(script, "rb").readline().decode(errors="replace")
    cands = [first[2:].split()[0]] if first.startswith("#!") else []
    cands += [shutil.which(f"python3.{v}") for v in (13, 12, 11, 10)]
    for py in filter(None, cands):
        if subprocess.run([py, "-c", "import sabctools.sabctools"], capture_output=True).returncode == 0:
            return py
    sys.exit("SABnzbd está instalado pero a su Python le falta sabctools (pip install sabctools)")


CLIENTS = {"nebula": Nebula, "sabnzbd": Sabnzbd, "nzbget": Nzbget}


# ---------------------------------------------------------------------------

def prepare(work, size_mb):
    src = os.path.join(work, "src")
    stamp = os.path.join(work, f"case-{size_mb}.json")
    if os.path.exists(stamp):
        return src, json.load(open(stamp))
    shutil.rmtree(src, ignore_errors=True)
    os.makedirs(src)
    raw = os.path.join(work, TITLE + ".mkv")
    log(f"Creando {size_mb} MB de vídeo aleatorio, 7z por volúmenes de 50 MB y PAR2 al 5 %…")
    with open(raw, "wb") as f:
        for _ in range(size_mb):
            f.write(os.urandom(1_000_000))
    info = {"size": os.path.getsize(raw), "md5": md5(raw)}
    run(["7z", "a", "-mx0", "-v50m", os.path.join(src, TITLE + ".7z"), raw])
    os.remove(raw)
    vols = sorted(os.listdir(src))
    run(["par2", "create", "-q", "-r5", "-n4", TITLE + ".par2"] + vols, cwd=src)
    info["files"] = len(os.listdir(src))
    info["bytes"] = sum(os.path.getsize(os.path.join(src, f)) for f in os.listdir(src))
    json.dump(info, open(stamp, "w"))
    return src, info


def bench_one(cls, args, work, nzb, mock, info):
    case = os.path.join(work, cls.name.lower())
    shutil.rmtree(case, ignore_errors=True)
    os.makedirs(case)
    c = cls(args)
    base = mock.mark()
    t0 = time.monotonic()
    pid, out = c.start(case, nzb, args.conns)
    sampler = Sampler(pid)
    sampler.start()
    try:
        c.wait(args.timeout)
        t_end = time.monotonic()
    finally:
        cpu, rss = sampler.stop()
        version = c.version()
        c.stop()
    got, first, last, peak, active = mock.window(t0, base)
    dl = max(active, 1e-3)
    extracted = getattr(c, "unpack", True)
    path = find_file(out, info["size"])
    ok = (bool(path) and md5(path) == info["md5"]) if extracted else None
    total = t_end - t0
    res = {
        "client": cls.name, "version": version, "ok": ok,
        "avg_mbs": round(got / dl / 1e6, 1), "peak_mbs": round(peak, 1),
        "download_s": round(dl, 1), "total_s": round(total, 1), "post_s": round(max(total - (last - t0), 0), 1),
        "cpu_s": round(cpu, 1), "cpu_pct": round(100 * cpu / total), "ram_mb": round(rss / 1e6),
        "received_mb": round(got / 1e6), "extracted": extracted,
    }
    if not args.keep:
        shutil.rmtree(case, ignore_errors=True)
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--size-mb", type=int, default=1000)
    ap.add_argument("--conns", type=int, default=20)
    ap.add_argument("--missing", type=int, default=6, help="artículos de datos que faltan (se reparan con PAR2)")
    ap.add_argument("--clients", default="nebula,sabnzbd,nzbget")
    ap.add_argument("--runs", type=int, default=1, help="repeticiones por cliente (se queda con la mediana del tiempo total)")
    ap.add_argument("--work", default="/tmp/nebula-bench")
    ap.add_argument("--timeout", type=int, default=1800)
    ap.add_argument("--json", help="guardar los resultados en este archivo")
    ap.add_argument("--keep", action="store_true", help="no borrar lo descargado")
    args = ap.parse_args()

    for tool in ("par2", "7z"):
        if not shutil.which(tool):
            sys.exit(f"Falta {tool}")
    log("Compilando nebula-cli y nntp-bench (release)…")
    subprocess.run(["cargo", "build", "--release", "-q", "-p", "nebula-cli", "-p", "nntp-bench"], cwd=ROOT, check=True)

    os.makedirs(args.work, exist_ok=True)
    src, info = prepare(args.work, args.size_mb)
    nzb = os.path.join(args.work, TITLE + ".nzb")
    log(f"Caso: {info['files']} archivos, {info['bytes'] / 1e6:.0f} MB, faltan {args.missing} artículos. {args.conns} conexiones.")
    mock = Mock(os.path.join(ROOT, "target", "release", "nntp-bench"), src, nzb, args.conns, args.missing)

    results = []
    try:
        for key in args.clients.split(","):
            cls = CLIENTS[key.strip()]
            if cls is not Nebula and not cls.bin:
                log(f"{cls.name} no está instalado: se salta.")
                continue
            runs = []
            for i in range(args.runs):
                log(f"{cls.name}: pasada {i + 1}/{args.runs}…")
                try:
                    runs.append(bench_one(cls, args, args.work, nzb, mock, info))
                    log("  " + json.dumps(runs[-1]))
                except Exception as e:
                    log(f"  falló: {e!r}")
            if runs:
                runs.sort(key=lambda r: r["total_s"])
                results.append(runs[len(runs) // 2])
    finally:
        mock.close()

    meta = {"cpu": cpu_name(), "cores": os.cpu_count(), "size_mb": args.size_mb, "conns": args.conns, "missing": args.missing}
    print(f"\n{meta['cpu']}, {meta['cores']} núcleos · {info['bytes'] / 1e6:.0f} MB · {args.conns} conexiones · {args.missing} artículos que reparar\n")
    print("| Cliente | Media | Pico | Descarga | Total | Post-proceso | CPU | RAM máx. | OK |")
    print("|---|---|---|---|---|---|---|---|---|")
    for r in results:
        print(f"| {r['client']} {r['version']} | {r['avg_mbs']} MB/s | {r['peak_mbs']} MB/s | {r['download_s']} s | {r['total_s']} s | {r['post_s']} s "
              f"| {r['cpu_s']} s ({r['cpu_pct']} %) | {r['ram_mb']} MB | {'sí' if r['ok'] else 'solo descarga' if r['ok'] is None else 'NO'} |")
    if args.json:
        json.dump({"meta": meta, "results": results}, open(args.json, "w"), indent=2)


def cpu_name():
    try:
        for line in open("/proc/cpuinfo"):
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return "?"


if __name__ == "__main__":
    main()
