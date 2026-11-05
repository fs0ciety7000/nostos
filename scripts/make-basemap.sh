#!/usr/bin/env bash
# Builds tiles/nostos-basemap.pmtiles: the Protomaps planet cut to the regions
# in tiles/regions.geojson (Québec and Lisbonne for now), then shows how to
# upload it to R2. Low zooms (0-5) cover the whole world, since the tiles that
# intersect the regions at those zooms are world-sized.
#
# Requires the official pmtiles CLI: https://github.com/protomaps/go-pmtiles
# (brew install pmtiles, the release binary, or: go install github.com/protomaps/go-pmtiles@latest).
# Max zoom 14 gives ~500 MB for the current regions (15: ~1 GB); MapLibre
# overzooms vector tiles beyond it.
set -euo pipefail
cd "$(dirname "$0")/.."
BUILD=${BUILD:-$(curl -fsS https://build-metadata.protomaps.dev/builds.json | python3 -c "import sys,json;print(json.load(sys.stdin)[-1]['key'])")}
MAXZOOM=${MAXZOOM:-14}
echo "Extracting $BUILD (max zoom $MAXZOOM)…"
pmtiles extract "https://build.protomaps.com/$BUILD" tiles/nostos-basemap.pmtiles \
  --region=tiles/regions.geojson --maxzoom="$MAXZOOM" --download-threads=8
pmtiles show tiles/nostos-basemap.pmtiles | head -20
cat <<MSG

Next: upload to the R2 bucket served on your media domain, for example
  rclone copyto tiles/nostos-basemap.pmtiles r2:nostos-media/tiles/nostos-basemap.pmtiles
then set PUBLIC_BASEMAP_URL=https://<media domain>/tiles/nostos-basemap.pmtiles for the web app.
The bucket needs a CORS rule allowing GET with the Range header from the web app's origin.
MSG
