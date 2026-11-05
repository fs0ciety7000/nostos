#!/usr/bin/env python3
"""Seeds a running nostos-api with two demo trips (Québec, Lisbonne).

The photos are the embedded previews of public CC0 Fujifilm RAF samples
(raw.pixls.us), downloaded by `scripts/fetch-sample-previews.py`. exiftool
rewrites their position, capture time and film simulation so they tell a trip.
The trip titles start with "Démo" so they are never mistaken for real ones.

usage: NOSTOS_API_URL=http://127.0.0.1:8080 NOSTOS_OWNER_TOKEN=... scripts/seed-demo.py
Requires exiftool.
"""
import json, os, pathlib, shutil, subprocess, sys, tempfile, urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
PREVIEWS = ROOT / "samples-raw" / "previews"
API = os.environ.get("NOSTOS_API_URL", "http://127.0.0.1:8080").rstrip("/")
OWNER = os.environ.get("NOSTOS_OWNER_TOKEN")

# FilmMode / Saturation codes (exiftool FujiFilm.pm).
FILM = {"Classic Chrome": (0x600, 0), "Eterna": (0x700, 0), "Nostalgic Neg.": (0xa00, 0), "Velvia": (0x200, 0),
        "Classic Neg.": (0x800, 0), "Reala Ace": (0xb00, 0), "Acros+R": (0x000, 0x501)}

TRIPS = [
    ("quebec-2026", "Démo · L'été indien", "-04:00", [
        ("Montréal", 45.5019, -73.5674, "2026:09:12", ["19:10", "19:35", "20:05"], "Nostalgic Neg."),
        ("Vieux-Québec", 46.8123, -71.2075, "2026:09:14", ["17:40", "18:05", "18:30", "19:00"], "Classic Chrome"),
        ("Chutes Montmorency", 46.8906, -71.1478, "2026:09:15", ["10:20", "10:45", "11:10", "11:30"], "Classic Chrome"),
        ("Tadoussac", 48.1430, -69.7160, "2026:09:17", ["07:10", "07:50", "08:35"], "Eterna"),
        ("Percé", 48.5244, -64.2128, "2026:09:19", ["15:00", "15:40", "16:30", "18:50"], "Velvia"),
    ]),
    ("lisbonne-2027", "Démo · Lisbonne", "+01:00", [
        ("Alfama", 38.7114, -9.1300, "2027:04:16", ["08:40", "09:10", "19:50"], "Reala Ace"),
        ("Belém", 38.6916, -9.2160, "2027:04:17", ["18:30", "19:20"], "Classic Neg."),
        ("Sintra", 38.7876, -9.3904, "2027:04:18", ["09:15", "09:40", "10:05"], "Acros+R"),
    ]),
]


def call(method, path, body=None, raw=None, url=None):
    headers = {}
    if not url:
        headers["Authorization"] = f"Bearer {OWNER}"
    if body is not None:
        raw = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url or API + path, data=raw, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            data = r.read()
            return r.status, json.loads(data) if data else None
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"null")


def main():
    if not OWNER:
        sys.exit("NOSTOS_OWNER_TOKEN is not set")
    if not shutil.which("exiftool"):
        sys.exit("exiftool is required")
    images = sorted(PREVIEWS.glob("*.jpg"))
    if not images:
        sys.exit(f"no previews in {PREVIEWS}: run scripts/fetch-sample-previews.py first")
    nostos = ROOT / "target" / "release" / "nostos"
    if not nostos.exists():
        sys.exit(f"{nostos} is missing: run cargo build --release -p nostos-cli first")
    tmp = pathlib.Path(tempfile.mkdtemp())
    n = 0
    for slug, title, offset, stops in TRIPS:
        folder = tmp / slug
        folder.mkdir()
        for s_i, (place, lat, lon, day, times, film) in enumerate(stops):
            for t_i, hm in enumerate(times):
                src = images[n % len(images)]
                n += 1
                dst = folder / f"DSCF{2100 + n:04d}.JPG"
                shutil.copy(src, dst)
                d = 0.0012 * t_i  # a few hundred metres between shots
                mode, sat = FILM[film]
                subprocess.run(["exiftool", "-q", "-overwrite_original", "-n",
                                f"-GPSLatitude={lat + d}", f"-GPSLatitudeRef={'N' if lat >= 0 else 'S'}",
                                f"-GPSLongitude={abs(lon - d)}", f"-GPSLongitudeRef={'W' if lon < 0 else 'E'}",
                                f"-DateTimeOriginal={day} {hm}:00", f"-OffsetTimeOriginal={offset}",
                                f"-FujiFilm:FilmMode={mode}", f"-FujiFilm:Saturation={sat}", str(dst)], check=True)
        # The real importer does the sending, so the demo exercises it.
        subprocess.run([str(nostos), "import", str(folder), "--trip", slug, "--title", title, "--wait"], check=True)
        again = subprocess.run([str(nostos), "import", str(folder), "--trip", slug], check=True, capture_output=True, text=True)
        if "nothing to send" not in again.stdout:
            sys.exit(f"a second import of {slug} should send nothing, it printed:\n{again.stdout}")
        print(f"{slug}: second import skipped every file")
    shutil.rmtree(tmp)
    print(f"{n} photos sent")
    # Name the stops once they exist (no reverse geocoding yet).
    import time
    for slug, _title, _offset, stops in TRIPS:
        for _ in range(240):
            status, trip = call("GET", f"/trips/{slug}")
            if status == 200 and trip["photos"]["in_flight"] == 0 and trip["stops"]:
                break
            time.sleep(0.5)
        found = [s for s in trip["stops"] if not s["is_waypoint"]]
        if len(found) != len(stops):
            print(f"{slug}: {len(found)} stops found, {len(stops)} expected; not naming them")
            continue
        for attempt in range(10):
            results = [call("PATCH", f"/stops/{st['id']}", {"name": place})[0] for st, (place, *_r) in zip(found, stops)]
            if all(r == 200 for r in results):
                break
            # A recomputation replaced the stops in between: fetch them again.
            time.sleep(0.5)
            found = [s for s in call("GET", f"/trips/{slug}")[1]["stops"] if not s["is_waypoint"]]
        print(f"{slug}: {len(found)} stops named")


if __name__ == "__main__":
    main()
