#!/usr/bin/env python3
"""Turns a JPEG or RAF (even truncated) into a tiny test fixture: a JPEG made
of a tiny 8x8 image carrying the original Exif APP1 segment, plus exiftool's numeric output as
the reference ("golden") JSON.

usage: scripts/make-fixture.py <photo> <fixture-name>
"""
import io, json, struct, subprocess, sys, pathlib
from PIL import Image

def app1(data: bytes) -> bytes:
    if data.startswith(b"FUJIFILMCCD-RAW"):
        off, length = struct.unpack(">II", data[84:92])
        data = data[off:off + length]
    assert data[:2] == b"\xff\xd8", "not a JPEG stream"
    p = 2
    while p + 4 <= len(data):
        marker, seg_len = struct.unpack(">HH", data[p:p + 4])
        if marker == 0xFFE1 and data[p + 4:p + 10] == b"Exif\0\0":
            return data[p:p + 2 + seg_len]
        if marker == 0xFFDA:
            break
        p += 2 + seg_len
    raise SystemExit("no Exif APP1 segment")

TAGS = ["Make", "Model", "LensModel", "DateTimeOriginal", "SubSecTimeOriginal", "OffsetTimeOriginal",
        "FNumber", "ExposureTime", "ISO", "FocalLength", "FocalLengthIn35mmFormat", "ExifImageWidth", "ExifImageHeight",
        "GPSLatitude", "GPSLongitude", "GPSAltitude",
        "FilmMode", "Saturation", "GrainEffectRoughness", "GrainEffectSize", "ColorChromeEffect", "ColorChromeFXBlue",
        "ShadowTone", "HighlightTone", "Sharpness", "NoiseReduction", "Clarity", "WhiteBalance", "ColorTemperature",
        "WhiteBalanceFineTune", "DynamicRangeSetting", "DevelopmentDynamicRange", "DRangePriority",
        "DRangePriorityAuto", "DRangePriorityFixed", "BWAdjustment", "BWMagentaGreen"]

src, name = sys.argv[1], sys.argv[2]
out = pathlib.Path(__file__).resolve().parent.parent / "crates/nostos-exif/tests/fixtures"
jpg = out / f"{name}.jpg"
buf = io.BytesIO(); Image.new("L", (8, 8), 128).save(buf, "JPEG", quality=50)
tiny = buf.getvalue()
jpg.write_bytes(tiny[:2] + app1(pathlib.Path(src).read_bytes()) + tiny[2:])
args = ["exiftool", "-j", "-n", "-G1", *[f"-{t}" for t in TAGS], str(jpg)]
golden = json.loads(subprocess.check_output(args))[0]
golden.pop("SourceFile", None)
golden["_exiftool"] = subprocess.check_output(["exiftool", "-ver"]).decode().strip()
(out / f"{name}.json").write_text(json.dumps(golden, indent=1, ensure_ascii=False) + "\n")
print(jpg.name, jpg.stat().st_size, "bytes")
