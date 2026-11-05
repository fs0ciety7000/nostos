#!/usr/bin/env python3
"""Downloads the embedded JPEG preview of one public CC0 RAF per Fujifilm body
from raw.pixls.us into samples-raw/previews/ (about 40 MB in total). Only the
preview byte range is fetched, never the raw sensor data."""
import pathlib, re, struct, urllib.request

BODIES = ["X-T5", "X100VI", "X-H2", "X-S20", "X-T4", "X100V", "X-Pro3", "X-E4", "X-T30%20II", "GFX100S"]
OUT = pathlib.Path(__file__).resolve().parent.parent / "samples-raw" / "previews"


def get(url, rng=None):
    req = urllib.request.Request(url, headers={"Range": f"bytes={rng[0]}-{rng[1]}"} if rng else {})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


OUT.mkdir(parents=True, exist_ok=True)
for body in BODIES:
    dst = OUT / (body.replace("%20", "_") + ".jpg")
    if dst.exists():
        continue
    name = re.search(r'href="([^"]+\.RAF)"', get(f"https://raw.pixls.us/data/Fujifilm/{body}/").decode()).group(1)
    url = f"https://raw.pixls.us/download/data/Fujifilm/{body}/{name}"
    off, length = struct.unpack(">II", get(url, (0, 159))[84:92])
    jpg = get(url, (off, off + length - 1))
    assert jpg[:2] == b"\xff\xd8", f"{body}: preview is not a JPEG"
    dst.write_bytes(jpg)
    print(dst.name, len(jpg) // 1024, "KB")
