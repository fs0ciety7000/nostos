//! Background ingestion of uploaded photos.
//!
//! The queue is the `photos` table itself: rows in `pending` are claimed with
//! `FOR UPDATE SKIP LOCKED`, so several workers (or several API instances)
//! never process the same photo twice. When the last pending photo of a trip
//! is done, its stops are recomputed.

use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime, TimeDelta};
use nostos_exif::{Container, Metadata};
use uuid::Uuid;

use crate::{imaging, stops, AppState};

#[derive(sqlx::FromRow)]
struct Job {
    id: Uuid,
    trip_id: Uuid,
    original_key: String,
    byte_size: i64,
}

#[derive(Debug, thiserror::Error)]
enum IngestError {
    #[error("storage: {0}")]
    Storage(#[from] object_store::Error),
    #[error("metadata: {0}")]
    Exif(#[from] nostos_exif::Error),
    #[error("image: {0}")]
    Image(#[from] imaging::ImagingError),
    #[error("TIFF files are not supported yet")]
    Tiff,
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("worker crashed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

/// Starts `n` background workers. They poll every second when idle.
pub fn spawn_workers(state: &AppState, n: usize) {
    for i in 0..n {
        let state = state.clone();
        tokio::spawn(async move {
            tracing::info!(worker = i, "ingest worker started");
            loop {
                match drain(&state).await {
                    Ok(0) => tokio::time::sleep(Duration::from_secs(1)).await,
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(error = %e, "ingest worker error");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });
    }
}

/// Processes pending photos until the queue is empty. As soon as a trip
/// has no photo left in flight, its stops are recomputed, without waiting
/// for other trips in the queue. Returns how many photos were processed.
pub async fn drain(state: &AppState) -> Result<usize, sqlx::Error> {
    let mut n = 0;
    while let Some(job) = claim(state).await? {
        n += 1;
        let id = job.id;
        if let Err(e) = process(state, &job).await {
            tracing::warn!(photo = %id, error = %e, "photo failed");
            sqlx::query(
                "update photos set status = 'failed', error = $2, updated_at = now() where id = $1",
            )
            .bind(id)
            .bind(e.to_string())
            .execute(&state.db)
            .await?;
        }
        recompute_if_settled(state, job.trip_id).await?;
    }
    Ok(n)
}

async fn recompute_if_settled(state: &AppState, trip: Uuid) -> Result<(), sqlx::Error> {
    let busy: bool = sqlx::query_scalar(
        "select exists(select 1 from photos where trip_id = $1 and status in ('pending', 'processing'))",
    )
    .bind(trip)
    .fetch_one(&state.db)
    .await?;
    if !busy {
        let count = stops::recompute(&state.db, trip).await?;
        tracing::info!(trip = %trip, stops = count, "stops recomputed");
    }
    Ok(())
}

async fn claim(state: &AppState) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as(
        r"
        update photos set status = 'processing', attempts = attempts + 1, updated_at = now()
        where id = (select id from photos where status = 'pending' order by created_at for update skip locked limit 1)
        returning id, trip_id, original_key, byte_size
        ",
    )
    .fetch_optional(&state.db)
    .await
}

fn naive(dt: nostos_exif::DateTime) -> Option<NaiveDateTime> {
    let d = NaiveDate::from_ymd_opt(i32::from(dt.year), u32::from(dt.month), u32::from(dt.day))?;
    d.and_hms_milli_opt(
        u32::from(dt.hour),
        u32::from(dt.minute),
        u32::from(dt.second.min(59)),
        u32::from(dt.millis.unwrap_or(0)),
    )
}

/// UTC offset of the camera clock: from `OffsetTimeOriginal`, else from the
/// GPS fix time (rounded to 15 minutes, the finest real-world time-zone step).
fn utc_offset_minutes(m: &Metadata) -> Option<i16> {
    if m.utc_offset_minutes.is_some() {
        return m.utc_offset_minutes;
    }
    let local = naive(m.captured_at?)?;
    let utc = naive(m.gps?.timestamp_utc?)?;
    let minutes = (local - utc).num_minutes();
    let rounded = ((minutes as f64 / 15.0).round() * 15.0) as i64;
    ((-14 * 60..=14 * 60).contains(&rounded) && (minutes - rounded).abs() <= 5)
        .then_some(rounded as i16)
}

fn exposure_text(r: nostos_exif::Rational) -> Option<String> {
    if r.den == 0 || r.num <= 0 {
        return None;
    }
    Some(if r.num >= r.den {
        let s = r.num as f64 / r.den as f64;
        format!("{}\"", (s * 10.0).round() / 10.0)
    } else {
        format!("1/{}", (r.den as f64 / r.num as f64).round())
    })
}

async fn process(state: &AppState, job: &Job) -> Result<(), IngestError> {
    let size = job.byte_size as u64;
    let head = state
        .storage
        .get_range(
            &job.original_key,
            0..size.min(nostos_exif::HEADER_LEN as u64),
        )
        .await?;
    let (container, jpeg) = match nostos_exif::detect(&head)? {
        Container::Raf => {
            // Only the embedded preview is downloaded, never the sensor data.
            let (off, len) = nostos_exif::raf_preview_range(&head)?;
            let end = off.saturating_add(len).min(size);
            (
                Container::Raf,
                state.storage.get_range(&job.original_key, off..end).await?,
            )
        }
        Container::Jpeg => (Container::Jpeg, state.storage.get(&job.original_key).await?),
        Container::Tiff => return Err(IngestError::Tiff),
    };
    let meta = nostos_exif::read(&jpeg)?;

    let orientation = meta.orientation;
    let derived =
        tokio::task::spawn_blocking(move || imaging::derive(&jpeg, orientation)).await??;

    let base = format!("trips/{}/{}", job.trip_id, job.id);
    let thumb_key = format!("{base}/thumb.jpg");
    let preview_key = format!("{base}/preview.jpg");
    state
        .storage
        .put(&thumb_key, derived.thumb_jpeg.into())
        .await?;
    state
        .storage
        .put(&preview_key, derived.preview_jpeg.into())
        .await?;

    let local = meta.captured_at.and_then(naive);
    let offset = utc_offset_minutes(&meta);
    // Without an offset the wall clock is stored as if it were UTC: order
    // within the trip is still right, which is what stop detection needs.
    let captured_at =
        local.map(|l| (l - TimeDelta::minutes(i64::from(offset.unwrap_or(0)))).and_utc());
    let film = meta
        .fuji
        .as_ref()
        .and_then(|f| f.film_simulation)
        .map(|f| f.name());
    let recipe = meta
        .fuji
        .as_ref()
        .map(|f| serde_json::to_value(f).unwrap_or_default());
    let gps = meta.gps;

    sqlx::query(
        r"
        update photos set
            status = 'ready', error = null, updated_at = now(),
            container = $2, camera = $3, lens = $4,
            captured_at = $5, captured_local = $6, utc_offset_min = $7,
            f_number = $8, exposure_time = $9, iso = $10, focal_mm = $11, focal_35mm = $12,
            width = $13, height = $14, film_simulation = $15, recipe = $16,
            geom = case when $17::float8 is null then null else ST_SetSRID(ST_MakePoint($18, $17), 4326)::geography end,
            altitude_m = $19, gps_source = case when $17::float8 is null then null else 'exif' end,
            thumb_key = $20, preview_key = $21, thumbhash = $22, palette = $23
        where id = $1
        ",
    )
    .bind(job.id)
    .bind(if container == Container::Raf { "raf" } else { "jpeg" })
    .bind(&meta.model)
    .bind(&meta.lens_model)
    .bind(captured_at)
    .bind(local)
    .bind(offset)
    .bind(meta.exposure.f_number.map(|v| v as f32))
    .bind(meta.exposure.exposure_time.and_then(exposure_text))
    .bind(meta.exposure.iso.and_then(|v| i32::try_from(v).ok()))
    .bind(meta.exposure.focal_length_mm.map(|v| v as f32))
    .bind(meta.exposure.focal_length_35mm.and_then(|v| i16::try_from(v).ok()))
    .bind(i32::try_from(derived.width).ok())
    .bind(i32::try_from(derived.height).ok())
    .bind(film)
    .bind(recipe)
    .bind(gps.map(|g| g.latitude))
    .bind(gps.map(|g| g.longitude))
    .bind(gps.and_then(|g| g.altitude_m).map(|v| v as f32))
    .bind(&thumb_key)
    .bind(&preview_key)
    .bind(&derived.thumbhash)
    .bind(&derived.palette)
    .execute(&state.db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_texts() {
        assert_eq!(
            exposure_text(nostos_exif::Rational { num: 1, den: 250 }).as_deref(),
            Some("1/250")
        );
        assert_eq!(
            exposure_text(nostos_exif::Rational { num: 10, den: 2500 }).as_deref(),
            Some("1/250")
        );
        assert_eq!(
            exposure_text(nostos_exif::Rational { num: 25, den: 10 }).as_deref(),
            Some("2.5\"")
        );
        assert_eq!(
            exposure_text(nostos_exif::Rational { num: 0, den: 1 }),
            None
        );
    }
}
