#!/usr/bin/env bash
# Full-stack end-to-end run: throw-away PostGIS database, nostos-api with
# local storage, demo seed, production build of the web app, Playwright.
# Requires: PostgreSQL with PostGIS (NOSTOS_E2E_ADMIN_URL), exiftool, Node.
set -euo pipefail
cd "$(dirname "$0")/.."
ADMIN_URL=${NOSTOS_E2E_ADMIN_URL:-postgres://nostos:nostos@localhost/postgres}
DB=nostos_e2e_$$
WORK=$(mktemp -d)
API_PORT=${API_PORT:-8098}
WEB_PORT=${WEB_PORT:-4173}
pids=()
cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  psql "$ADMIN_URL" -qc "drop database if exists $DB with (force)" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# A random owner credential for this run only, never printed.
OWNER_SECRET=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')
psql "$ADMIN_URL" -qc "create database $DB" >/dev/null
python3 scripts/fetch-sample-previews.py
cargo build -q --release -p nostos-api -p nostos-cli

DATABASE_URL="${ADMIN_URL%/*}/$DB" NOSTOS_OWNER_TOKEN="$OWNER_SECRET" NOSTOS_STORAGE_DIR="$WORK/media" \
  NOSTOS_BIND="127.0.0.1:$API_PORT" NOSTOS_PUBLIC_URL="http://127.0.0.1:$API_PORT" \
  ./target/release/nostos-api >"$WORK/api.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$API_PORT/health" >/dev/null && break; sleep 0.2; done

NOSTOS_API_URL="http://127.0.0.1:$API_PORT" NOSTOS_OWNER_TOKEN="$OWNER_SECRET" python3 scripts/seed-demo.py > "$WORK/seed.log" 2>&1 || { cat "$WORK/seed.log"; exit 1; }
grep -E "photos \(|sent,|processed:|second import" "$WORK/seed.log"
for _ in $(seq 120); do
  busy=$(psql "${ADMIN_URL%/*}/$DB" -tAc "select count(*) from photos where status in ('pending','processing')")
  [ "$busy" = 0 ] && break; sleep 0.5
done
echo "seeded: $(psql "${ADMIN_URL%/*}/$DB" -tAc "select count(*) from photos where status='ready'") photos ready"

(cd web && npm run -s build >/dev/null)
# Basemap: the local extract when it exists (scripts/make-basemap.sh), else the planet relay.
if [ -f tiles/nostos-basemap.pmtiles ]; then export BASEMAP_FILE="$PWD/tiles/nostos-basemap.pmtiles"; echo "basemap: local extract"; fi
(cd web && NODE_USE_ENV_PROXY=1 BASEMAP_UPSTREAM="${BASEMAP_UPSTREAM:-https://build.protomaps.com/20261104.pmtiles}" NOSTOS_API_URL="http://127.0.0.1:$API_PORT" PORT=$WEB_PORT HOST=127.0.0.1 ORIGIN="http://127.0.0.1:$WEB_PORT" exec node build >"$WORK/web.log" 2>&1) &
pids+=($!)
for _ in $(seq 50); do curl -fs -o /dev/null "http://127.0.0.1:$WEB_PORT/login" && break; sleep 0.2; done

(cd web && E2E_BASE_URL="http://127.0.0.1:$WEB_PORT" E2E_OWNER_SECRET="$OWNER_SECRET" npx playwright test "$@") || {
  echo "--- api.log (tail)"; tail -20 "$WORK/api.log"; echo "--- web.log (tail)"; tail -20 "$WORK/web.log"; exit 1; }
