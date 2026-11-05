//! HTTP routes. Everything but `/health` and `/media` needs the owner token.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::{ingest, stops, AppState};

/// Largest original accepted (a 102 MP GFX RAF is ~210 MB).
pub const MAX_UPLOAD_BYTES: i64 = 300 * 1024 * 1024;

// ---------- auth ----------

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub async fn require_owner(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| constant_time_eq(t.trim().as_bytes(), state.owner_token.as_bytes()));
    if ok {
        next.run(req).await
    } else {
        ApiError::Unauthorized.into_response()
    }
}

// ---------- health & media ----------

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let db = sqlx::query_scalar::<_, i32>("select 1")
        .fetch_one(&state.db)
        .await
        .is_ok();
    let status = if db {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({ "ok": db, "version": env!("CARGO_PKG_VERSION") })),
    )
}

/// Serves thumbnails and previews from local storage. Originals stay private.
pub async fn media(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Response> {
    let public = key.starts_with("trips/")
        && (key.ends_with("/thumb.jpg") || key.ends_with("/preview.jpg"))
        && !key.contains("..");
    if !state.storage.is_local() || !public {
        return Err(ApiError::NotFound("media"));
    }
    let data = state.storage.get(&key).await.map_err(|e| match e {
        object_store::Error::NotFound { .. } => ApiError::NotFound("media"),
        other => ApiError::Storage(other),
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        data,
    )
        .into_response())
}

// ---------- trips ----------

#[derive(Deserialize)]
pub struct NewTrip {
    slug: String,
    title: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Trip {
    id: Uuid,
    slug: String,
    title: String,
    created_at: DateTime<Utc>,
}

pub async fn create_trip(
    State(state): State<AppState>,
    Json(body): Json<NewTrip>,
) -> ApiResult<(StatusCode, Json<Trip>)> {
    let slug = body.slug.trim().to_lowercase();
    let valid = !slug.is_empty()
        && slug.len() <= 64
        && slug.split('-').all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
    if !valid {
        return Err(ApiError::BadRequest("slug must be lowercase letters and digits separated by single hyphens, e.g. quebec-2026".into()));
    }
    let title = body.title.trim();
    if title.is_empty() || title.len() > 200 {
        return Err(ApiError::BadRequest(
            "title must be 1 to 200 characters".into(),
        ));
    }
    let trip = sqlx::query_as::<_, Trip>(
        "insert into trips (slug, title) values ($1, $2) returning id, slug, title, created_at",
    )
    .bind(&slug)
    .bind(title)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            ApiError::Conflict(format!("a trip with slug {slug} already exists"))
        }
        _ => ApiError::Db(e),
    })?;
    Ok((StatusCode::CREATED, Json(trip)))
}

async fn trip_by_slug(state: &AppState, slug: &str) -> ApiResult<Trip> {
    sqlx::query_as::<_, Trip>("select id, slug, title, created_at from trips where slug = $1")
        .bind(slug)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound("trip"))
}

pub async fn list_trips(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows = sqlx::query(
        r"
        select t.id, t.slug, t.title, t.created_at,
               count(p.id) filter (where p.status = 'ready')::int as photos,
               min(p.captured_local)::date as starts_on, max(p.captured_local)::date as ends_on
        from trips t left join photos p on p.trip_id = t.id
        group by t.id order by min(p.captured_local) nulls last, t.created_at
        ",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(Value::Array(
        rows.iter()
            .map(|r| {
                json!({
                    "id": r.get::<Uuid, _>("id"), "slug": r.get::<String, _>("slug"), "title": r.get::<String, _>("title"),
                    "photos": r.get::<i32, _>("photos"),
                    "starts_on": r.get::<Option<NaiveDate>, _>("starts_on"), "ends_on": r.get::<Option<NaiveDate>, _>("ends_on"),
                })
            })
            .collect(),
    )))
}

pub async fn get_trip(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let trip = trip_by_slug(&state, &slug).await?;

    let counts = sqlx::query(
        r"
        select count(*) filter (where status = 'ready')::int as ready,
               count(*) filter (where status in ('uploading', 'pending', 'processing'))::int as in_flight,
               count(*) filter (where status = 'failed')::int as failed,
               count(*) filter (where status = 'ready' and geom is not null)::int as with_gps
        from photos where trip_id = $1
        ",
    )
    .bind(trip.id)
    .fetch_one(&state.db)
    .await?;

    let films: Vec<(String, i64)> = sqlx::query_as(
        "select film_simulation, count(*) from photos where trip_id = $1 and status = 'ready' and film_simulation is not null group by 1 order by 2 desc, 1",
    )
    .bind(trip.id)
    .fetch_all(&state.db)
    .await?;

    let days: Vec<(NaiveDate, i64)> = sqlx::query_as(
        "select captured_local::date, count(*) from photos where trip_id = $1 and status = 'ready' and captured_local is not null group by 1 order by 1",
    )
    .bind(trip.id)
    .fetch_all(&state.db)
    .await?;
    let first = days.first().map(|d| d.0);
    let days: Vec<Value> = days
        .iter()
        .map(|(date, n)| json!({ "chant": first.map(|f| (*date - f).num_days() + 1), "date": date, "photos": n }))
        .collect();

    let stops = stops_json(&state, trip.id).await?;
    let km: Option<f64> = sqlx::query_scalar(
        "select ST_Length(ST_MakeLine(geom::geometry order by seq)::geography) / 1000 from stops where trip_id = $1",
    )
    .bind(trip.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({
        "trip": trip,
        "photos": { "ready": counts.get::<i32, _>("ready"), "in_flight": counts.get::<i32, _>("in_flight"),
                    "failed": counts.get::<i32, _>("failed"), "with_gps": counts.get::<i32, _>("with_gps") },
        "film_simulations": films.iter().map(|(f, n)| json!({ "name": f, "photos": n })).collect::<Vec<_>>(),
        "days": days,
        "stops": stops,
        "route_km": km.map(|k| (k * 10.0).round() / 10.0),
    })))
}

async fn stops_json(state: &AppState, trip_id: Uuid) -> ApiResult<Vec<Value>> {
    let rows = sqlx::query(
        r"
        select s.id, s.seq, s.name, ST_Y(s.geom::geometry) as lat, ST_X(s.geom::geometry) as lon,
               s.arrived_at, s.left_at, s.photo_count, s.is_waypoint,
               (select count(*) from photos p where p.stop_id = s.id)::int as photos_total,
               (select p.film_simulation from photos p where p.stop_id = s.id and p.film_simulation is not null
                 group by 1 order by count(*) desc, 1 limit 1) as main_film,
               (select p.thumb_key from photos p where p.stop_id = s.id and p.thumb_key is not null order by p.captured_at limit 1) as cover_key
        from stops s where s.trip_id = $1 order by s.seq
        ",
    )
    .bind(trip_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"), "seq": r.get::<i32, _>("seq"), "name": r.get::<Option<String>, _>("name"),
                "lat": r.get::<f64, _>("lat"), "lon": r.get::<f64, _>("lon"),
                "arrived_at": r.get::<DateTime<Utc>, _>("arrived_at"), "left_at": r.get::<DateTime<Utc>, _>("left_at"),
                "photos": r.get::<i32, _>("photos_total"), "photos_with_gps": r.get::<i32, _>("photo_count"),
                "is_waypoint": r.get::<bool, _>("is_waypoint"),
                "film_simulation": r.get::<Option<String>, _>("main_film"),
                "cover_url": r.get::<Option<String>, _>("cover_key").map(|k| state.media_url(&k)),
            })
        })
        .collect())
}

pub async fn trip_route(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let trip = trip_by_slug(&state, &slug).await?;
    let coords: Vec<(f64, f64)> = sqlx::query_as("select ST_X(geom::geometry), ST_Y(geom::geometry) from stops where trip_id = $1 order by seq")
        .bind(trip.id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(json!({
        "type": "Feature",
        "geometry": { "type": "LineString", "coordinates": coords.iter().map(|(x, y)| [*x, *y]).collect::<Vec<_>>() },
        "properties": { "trip": trip.slug, "stops": coords.len() },
    })))
}

pub async fn recompute_stops(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let trip = trip_by_slug(&state, &slug).await?;
    let n = stops::recompute(&state.db, trip.id).await?;
    Ok(Json(json!({ "stops": n })))
}

// ---------- photos ----------

const PHOTO_COLUMNS: &str = r"
    id, trip_id, file_name, byte_size, status, error, container, camera, lens,
    captured_at, captured_local, utc_offset_min, f_number, exposure_time, iso, focal_mm, focal_35mm,
    width, height, film_simulation, recipe,
    ST_Y(geom::geometry) as lat, ST_X(geom::geometry) as lon, altitude_m, gps_source, stop_id,
    thumb_key, preview_key, thumbhash, palette";

#[derive(sqlx::FromRow)]
struct PhotoRow {
    id: Uuid,
    trip_id: Uuid,
    file_name: String,
    byte_size: i64,
    status: String,
    error: Option<String>,
    container: Option<String>,
    camera: Option<String>,
    lens: Option<String>,
    captured_at: Option<DateTime<Utc>>,
    captured_local: Option<NaiveDateTime>,
    utc_offset_min: Option<i16>,
    f_number: Option<f32>,
    exposure_time: Option<String>,
    iso: Option<i32>,
    focal_mm: Option<f32>,
    focal_35mm: Option<i16>,
    width: Option<i32>,
    height: Option<i32>,
    film_simulation: Option<String>,
    recipe: Option<Value>,
    lat: Option<f64>,
    lon: Option<f64>,
    altitude_m: Option<f32>,
    gps_source: Option<String>,
    stop_id: Option<Uuid>,
    thumb_key: Option<String>,
    preview_key: Option<String>,
    thumbhash: Option<Vec<u8>>,
    palette: Option<Vec<String>>,
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl PhotoRow {
    fn to_json(&self, state: &AppState) -> Value {
        json!({
            "id": self.id, "trip_id": self.trip_id, "file_name": self.file_name, "byte_size": self.byte_size,
            "status": self.status, "error": self.error, "container": self.container,
            "camera": self.camera, "lens": self.lens,
            "captured_at": self.captured_at, "captured_local": self.captured_local, "utc_offset_min": self.utc_offset_min,
            "f_number": self.f_number, "exposure_time": self.exposure_time, "iso": self.iso,
            "focal_mm": self.focal_mm, "focal_35mm": self.focal_35mm, "width": self.width, "height": self.height,
            "film_simulation": self.film_simulation, "recipe": self.recipe,
            "lat": self.lat, "lon": self.lon, "altitude_m": self.altitude_m, "gps_source": self.gps_source,
            "stop_id": self.stop_id,
            "thumb_url": self.thumb_key.as_deref().map(|k| state.media_url(k)),
            "preview_url": self.preview_key.as_deref().map(|k| state.media_url(k)),
            "thumbhash": self.thumbhash.as_deref().map(base64),
            "palette": self.palette,
        })
    }

    fn to_feature(&self, state: &AppState) -> Value {
        let geometry = match (self.lon, self.lat) {
            (Some(x), Some(y)) => json!({ "type": "Point", "coordinates": [x, y] }),
            _ => Value::Null,
        };
        json!({ "type": "Feature", "id": self.id, "geometry": geometry, "properties": self.to_json(state) })
    }
}

pub async fn trip_photos(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let trip = trip_by_slug(&state, &slug).await?;
    let rows: Vec<PhotoRow> = sqlx::query_as(&format!(
        "select {PHOTO_COLUMNS} from photos where trip_id = $1 and status = 'ready' order by captured_at nulls last, file_name"
    ))
    .bind(trip.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        json!({ "type": "FeatureCollection", "features": rows.iter().map(|r| r.to_feature(&state)).collect::<Vec<_>>() }),
    ))
}

pub async fn get_photo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let row: PhotoRow =
        sqlx::query_as(&format!("select {PHOTO_COLUMNS} from photos where id = $1"))
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(ApiError::NotFound("photo"))?;
    Ok(Json(row.to_json(&state)))
}

#[derive(Deserialize)]
pub struct NewUpload {
    file_name: String,
    byte_size: i64,
}

pub async fn create_upload(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<NewUpload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let trip = trip_by_slug(&state, &slug).await?;
    let name = body.file_name.trim();
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let ext = match ext.as_deref() {
        Some("jpg" | "jpeg") => "jpg",
        Some("raf") => "raf",
        _ => {
            return Err(ApiError::BadRequest(format!(
                "{name}: only .jpg, .jpeg and .raf files are accepted"
            )))
        }
    };
    if name.is_empty() || name.len() > 255 || name.contains(['/', '\\']) {
        return Err(ApiError::BadRequest(
            "file_name must be a plain file name".into(),
        ));
    }
    if body.byte_size <= 0 || body.byte_size > MAX_UPLOAD_BYTES {
        return Err(ApiError::BadRequest(format!(
            "byte_size must be between 1 and {MAX_UPLOAD_BYTES}"
        )));
    }
    let id = Uuid::new_v4();
    let key = format!("trips/{}/{id}/original.{ext}", trip.id);
    sqlx::query("insert into photos (id, trip_id, file_name, byte_size, original_key) values ($1, $2, $3, $4, $5)")
        .bind(id)
        .bind(trip.id)
        .bind(name)
        .bind(body.byte_size)
        .bind(&key)
        .execute(&state.db)
        .await?;
    let target = state.storage.upload_target(&key, id).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "photo_id": id, "upload": target })),
    ))
}

async fn uploading_photo(state: &AppState, id: Uuid) -> ApiResult<(String, i64)> {
    let row: Option<(String, i64, String)> =
        sqlx::query_as("select original_key, byte_size, status from photos where id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (key, size, status) = row.ok_or(ApiError::NotFound("photo"))?;
    if status != "uploading" {
        return Err(ApiError::Conflict(format!("photo is already {status}")));
    }
    Ok((key, size))
}

/// Local-storage upload target. In production the client PUTs to R2 instead.
pub async fn put_content(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> ApiResult<StatusCode> {
    if !state.storage.is_local() {
        return Err(ApiError::NotFound("upload target"));
    }
    let (key, size) = uploading_photo(&state, id).await?;
    if body.len() as i64 != size {
        return Err(ApiError::BadRequest(format!(
            "expected {size} bytes, received {}",
            body.len()
        )));
    }
    state.storage.put(&key, body).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn complete_upload(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let (key, size) = uploading_photo(&state, id).await?;
    let stored = match state.storage.size(&key).await {
        Ok(s) => s,
        Err(object_store::Error::NotFound { .. }) => {
            return Err(ApiError::Conflict(
                "the file has not been uploaded yet".into(),
            ))
        }
        Err(e) => return Err(e.into()),
    };
    if stored != size as u64 {
        return Err(ApiError::Conflict(format!(
            "uploaded file has {stored} bytes, {size} were announced"
        )));
    }
    sqlx::query("update photos set status = 'pending', updated_at = now() where id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    if state.inline_ingest {
        ingest::drain(&state).await?;
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "photo_id": id, "status": "pending" })),
    ))
}

// ---------- stops ----------

#[derive(Deserialize)]
pub struct StopPatch {
    name: Option<String>,
}

pub async fn patch_stop(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<StopPatch>,
) -> ApiResult<Json<Value>> {
    let name = body
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if name.as_ref().is_some_and(|n| n.len() > 120) {
        return Err(ApiError::BadRequest(
            "name must be at most 120 characters".into(),
        ));
    }
    let done = sqlx::query("update stops set name = $2 where id = $1")
        .bind(id)
        .bind(&name)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound("stop"));
    }
    Ok(Json(json!({ "id": id, "name": name })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
