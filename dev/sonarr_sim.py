"""
Repite contra un Nebula real lo que hace Sonarr/Radarr con un cliente de descargas «SABnzbd»:
probar la conexión, mandar un NZB, seguirlo en la cola y esperar a que el historial lo dé por
terminado (con la carpeta final para importarlo). Solo usa la biblioteca estándar.

Uso:
  python dev/sonarr_sim.py --nzb archivo.nzb --key APIKEY [--url http://127.0.0.1:5077]
                           [--base /sabnzbd] [--cat tv] [--timeout 300] [--delete]

Termina con 0 si el NZB llega a Completed, 1 si falla y 2 si algo de la API no tiene la forma esperada.
"""
import argparse
import json
import os
import sys
import time
import urllib.parse
import urllib.request
import uuid


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://127.0.0.1:5077")
    ap.add_argument("--base", default="", help="url base, p. ej. /sabnzbd")
    ap.add_argument("--key", required=True)
    ap.add_argument("--nzb", required=True)
    ap.add_argument("--cat", default="tv")
    ap.add_argument("--timeout", type=int, default=300)
    ap.add_argument("--delete", action="store_true", help="borrar del historial al terminar (como tras importar)")
    a = ap.parse_args()
    api = f"{a.url.rstrip('/')}{a.base.rstrip('/')}/api"

    def call(mode, **params):
        q = urllib.parse.urlencode({"mode": mode, "apikey": a.key, "output": "json", **params})
        with urllib.request.urlopen(f"{api}?{q}", timeout=30) as r:
            return json.load(r)

    def need(cond, msg):
        if not cond:
            print(f"✗ {msg}")
            sys.exit(2)

    v = call("version")
    need("version" in v, f"version: {v}")
    print(f"✓ version {v['version']}")
    cfg = call("get_config")["config"]
    cats = [c["name"] for c in cfg["categories"]]
    print(f"✓ get_config: complete_dir={cfg['misc']['complete_dir']} categorías={cats}")
    need(a.cat in cats, f"la categoría {a.cat!r} no existe en Nebula (Sonarr también avisaría)")

    data = open(a.nzb, "rb").read()
    boundary = uuid.uuid4().hex
    body = (f"--{boundary}\r\nContent-Disposition: form-data; name=\"name\"; filename=\"{os.path.basename(a.nzb)}\"\r\n"
            "Content-Type: application/x-nzb\r\n\r\n").encode() + data + f"\r\n--{boundary}--\r\n".encode()
    q = urllib.parse.urlencode({"mode": "addfile", "cat": a.cat, "priority": -100, "apikey": a.key, "output": "json"})
    req = urllib.request.Request(f"{api}?{q}", data=body, headers={"Content-Type": f"multipart/form-data; boundary={boundary}"})
    with urllib.request.urlopen(req, timeout=60) as r:
        add = json.load(r)
    need(add.get("status") is True and add.get("nzo_ids"), f"addfile: {add}")
    nzo = add["nzo_ids"][0]
    print(f"✓ addfile → {nzo}")

    deadline = time.time() + a.timeout
    last = None
    while time.time() < deadline:
        queue = call("queue", start=0, limit=0)["queue"]
        slot = next((s for s in queue["slots"] if s["nzo_id"] == nzo), None)
        hist = call("history", start=0, limit=60, category=a.cat)["history"]
        h = next((s for s in hist["slots"] if s["nzo_id"] == nzo), None)
        state = f"queue {slot['status']} {slot['percentage']}% quedan {slot['timeleft']}" if slot else f"history {h['status']}" if h else "no aparece"
        if state != last:
            print(f"  · {state}")
            last = state
        if h and h["status"] == "Completed":
            need(h["storage"] and h["category"] == a.cat, f"history: {h}")
            print(f"✓ Completed: storage={h['storage']} bytes={h['bytes']} download_time={h['download_time']}s")
            if a.delete:
                call("history", name="delete", value=nzo, del_files=0)
                print("✓ borrado del historial")
            return 0
        if h and h["status"] == "Failed":
            print(f"✗ Failed: {h['fail_message']}")
            return 1
        time.sleep(1)
    print("✗ tiempo agotado")
    return 1


if __name__ == "__main__":
    sys.exit(main())
