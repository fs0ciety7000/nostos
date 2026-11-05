#!/usr/bin/env bash
# Builds fixtures with non-default Fujifilm settings by letting exiftool write
# them into copies of a real X-T5 / X100VI Exif block, then records exiftool's
# own reading as the golden JSON. Requires exiftool.
set -euo pipefail
cd "$(dirname "$0")/.."
FX=crates/nostos-exif/tests/fixtures
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

make() { # name base exiftool-args...
  local name=$1 base=$2; shift 2
  cp "$FX/$base.jpg" "$tmp/$name.jpg"
  exiftool -q -overwrite_original -n "$@" "$tmp/$name.jpg"
  python3 scripts/make-fixture.py "$tmp/$name.jpg" "$name"
}

make recipe-classic-chrome x-t5 -FilmMode=0x600 -FujiFilm:Saturation=0x80 -ShadowTone=-16 -HighlightTone=16 \
  -GrainEffectRoughness=32 -GrainEffectSize=16 -ColorChromeEffect=64 -ColorChromeFXBlue=32 \
  -FujiFilm:WhiteBalance=0 "-WhiteBalanceFineTune=40 -80" -DynamicRangeSetting=1 -DevelopmentDynamicRange=400 \
  -Clarity=-2000 -FujiFilm:NoiseReduction=0x280 -FujiFilm:Sharpness=0x82

make recipe-acros-red x-t5 -FujiFilm:Saturation=0x501 -ShadowTone=-32 -HighlightTone=-8 \
  -GrainEffectRoughness=64 -GrainEffectSize=32 -BWAdjustment=2 -BWMagentaGreen=-3 -DynamicRangeSetting=0

make recipe-nostalgic-kelvin x100vi -FilmMode=0xa00 -FujiFilm:Saturation=0x4c0 -ShadowTone=8 -HighlightTone=-48 \
  -FujiFilm:WhiteBalance=0xff0 -FujiFilm:ColorTemperature=5200 "-WhiteBalanceFineTune=-20 100" -ColorChromeEffect=32

make gps-montmorency x100vi -GPSLatitude=46.890612 -GPSLatitudeRef=N -GPSLongitude=71.147805 -GPSLongitudeRef=W \
  -GPSAltitude=52.4 -GPSAltitudeRef=0 -GPSDateStamp=2026:09:15 "-GPSTimeStamp=18:02:11" \
  "-DateTimeOriginal=2026:09:15 14:02:11" -OffsetTimeOriginal=-04:00 -SubSecTimeOriginal=25
