//! Stop detection ("escales").
//!
//! 1. Spatial clusters with `ST_ClusterDBSCAN` (eps in metres, corrected for
//!    the Web-Mercator scale at the trip's latitude).
//! 2. Chronological runs inside those clusters ("gaps and islands"): coming
//!    back to the same place a day later makes a new stop, and so does a gap of
//!    more than [`MAX_GAP_HOURS`] at one place.
//! 3. Photos without GPS join the stop closest in time, within
//!    [`TIME_MATCH_HOURS`].
//!
//! Hand-given stop names are carried over to the new stop nearest to the old one.

use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Photos closer than this belong to the same place.
pub const EPS_M: f64 = 1_200.0;
pub const MAX_GAP_HOURS: i32 = 10;
pub const TIME_MATCH_HOURS: i32 = 3;
/// A renamed stop keeps its name if the recomputed one is this close.
const NAME_CARRY_M: f64 = 2_000.0;

/// Takes the per-trip lock held for the whole transaction. Recomputation and
/// renaming both take it, so a rename never lands on a stop being replaced.
pub async fn lock_trip(tx: &mut sqlx::PgConnection, trip_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text, 7))")
        .bind(trip_id)
        .execute(tx)
        .await
        .map(|_| ())
}

#[allow(clippy::too_many_lines)] // one transaction, step by step
pub async fn recompute(db: &PgPool, trip_id: Uuid) -> Result<usize, sqlx::Error> {
    let mut tx = db.begin().await?;
    lock_trip(&mut tx, trip_id).await?;

    let lat: Option<f64> = sqlx::query_scalar(
        "select avg(ST_Y(geom::geometry)) from photos where trip_id = $1 and status = 'ready' and geom is not null",
    )
    .bind(trip_id)
    .fetch_one(&mut *tx)
    .await?;

    let named: Vec<(String, f64, f64)> = sqlx::query_as(
        "select name, ST_Y(geom::geometry), ST_X(geom::geometry) from stops where trip_id = $1 and name is not null",
    )
    .bind(trip_id)
    .fetch_all(&mut *tx)
    .await?;

    sqlx::query("update photos set stop_id = null where trip_id = $1")
        .bind(trip_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("delete from stops where trip_id = $1")
        .bind(trip_id)
        .execute(&mut *tx)
        .await?;

    let Some(lat) = lat else {
        tx.commit().await?;
        return Ok(0);
    };
    let eps_mercator = EPS_M / lat.to_radians().cos().max(0.05);

    let rows = sqlx::query(
        r"
        with pts as (
            select id, captured_at, geom,
                   ST_ClusterDBSCAN(ST_Transform(geom::geometry, 3857), eps => $2, minpoints => 1) over () as cluster
            from photos
            where trip_id = $1 and status = 'ready' and geom is not null and captured_at is not null
        ),
        marked as (
            select *, case
                when lag(cluster) over w is distinct from cluster then 1
                when captured_at - lag(captured_at) over w > make_interval(hours => $3) then 1
                else 0 end as starts
            from pts window w as (order by captured_at, id)
        ),
        runs as (
            select *, sum(starts) over (order by captured_at, id) as run from marked
        ),
        grouped as (
            select run,
                   row_number() over (order by min(captured_at), run)::int as seq,
                   min(captured_at) as arrived_at,
                   max(captured_at) as left_at,
                   count(*)::int as photo_count,
                   ST_Centroid(ST_Collect(geom::geometry))::geography as center,
                   array_agg(id) as photo_ids
            from runs group by run
        ),
        inserted as (
            insert into stops (trip_id, seq, geom, arrived_at, left_at, photo_count, is_waypoint)
            select $1, seq, center, arrived_at, left_at, photo_count,
                   photo_count < 3 and left_at - arrived_at < interval '20 minutes'
            from grouped
            returning id, seq
        )
        select i.id, g.photo_ids
        from inserted i join grouped g on g.seq = i.seq
        ",
    )
    .bind(trip_id)
    .bind(eps_mercator)
    .bind(MAX_GAP_HOURS)
    .fetch_all(&mut *tx)
    .await?;

    for row in &rows {
        let stop_id: Uuid = row.get(0);
        let ids: Vec<Uuid> = row.get(1);
        sqlx::query("update photos set stop_id = $1 where id = any($2)")
            .bind(stop_id)
            .bind(&ids)
            .execute(&mut *tx)
            .await?;
    }

    // Photos without a position join the stop nearest in time.
    sqlx::query(
        r"
        update photos p set stop_id = (
            select s.id from stops s
            where s.trip_id = p.trip_id
              and p.captured_at between s.arrived_at - make_interval(hours => $2) and s.left_at + make_interval(hours => $2)
            order by least(abs(extract(epoch from p.captured_at - s.arrived_at)), abs(extract(epoch from p.captured_at - s.left_at)))
            limit 1)
        where p.trip_id = $1 and p.status = 'ready' and p.geom is null and p.captured_at is not null
        ",
    )
    .bind(trip_id)
    .bind(TIME_MATCH_HOURS)
    .execute(&mut *tx)
    .await?;

    for (name, lat, lon) in named {
        sqlx::query(
            r"
            update stops set name = $2 where id = (
                select id from stops where trip_id = $1 and name is null
                  and ST_DWithin(geom, ST_SetSRID(ST_MakePoint($4, $3), 4326)::geography, $5)
                order by geom <-> ST_SetSRID(ST_MakePoint($4, $3), 4326)::geography limit 1)
            ",
        )
        .bind(trip_id)
        .bind(&name)
        .bind(lat)
        .bind(lon)
        .bind(NAME_CARRY_M)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(rows.len())
}
