#!/usr/bin/env bash
# Builds tiles/nostos-basemap.pmtiles: the Protomaps planet cut to the regions
# in tiles/regions.geojson (Québec and Lisbonne for now), then shows how to
# upload it to R2. Low zooms (0-5) cover the whole world, since the tiles that
# intersect the regions at those zooms are world-sized.
#
# Requires the official pmtiles CLI: https://github.com/protomaps/go-pmtiles
# (brew install pmtiles, or the release binary). About 10 minutes and ~300 MB.
set -euo pipefail
cd "$(dirname "$0")/.."
BUILD=${BUILD:-$(curl -fsS https://build-metadata.protomaps.dev/builds.json | python3 -c "import sys,json;print(json.load(sys.stdin)[-1]['key'])")}
MAXZOOM=${MAXZOOM:-15}
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
