//! End-to-end tests against a real `PostgreSQL` + `PostGIS`.
//!
//! Set `NOSTOS_TEST_DATABASE_URL` to a server URL whose role may create
//! databases, e.g. `postgres://nostos:nostos@localhost/postgres`. Each test
//! gets its own throw-away database. Without the variable the tests are
//! skipped (and say so).

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use nostos_api::config::StorageConfig;
use nostos_api::storage::Storage;
use nostos_api::{app, AppState, MIGRATOR};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const TOKEN: &str = "test-owner-token-0123456789abcdef0123";

struct Env {
    app: Router,
    admin_url: String,
    db_name: String,
    dir: TempDir,
}

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        // Best effort: drop the throw-away database on a separate runtime.
        let (url, name) = (self.admin_url.clone(), self.db_name.clone());
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                if let Ok(pool) = PgPoolOptions::new().max_connections(1).connect(&url).await {
                    let _ = sqlx::query(&format!("drop database if exists {name} with (force)"))
                        .execute(&pool)
                        .await;
                }
            });
        })
        .join();
    }
}

async fn setup() -> Option<Env> {
    let Ok(admin_url) = std::env::var("NOSTOS_TEST_DATABASE_URL") else {
        eprintln!("SKIPPED: set NOSTOS_TEST_DATABASE_URL to run the API integration tests");
        return None;
    };
    let db_name = format!("nostos_test_{}", uuid::Uuid::new_v4().simple());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await
        .expect("connect to NOSTOS_TEST_DATABASE_URL");
    sqlx::query(&format!("create database {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let (base, _) = admin_url.rsplit_once('/').unwrap();
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&format!("{base}/{db_name}"))
        .await
        .unwrap();
    MIGRATOR.run(&db).await.unwrap();

    let dir = std::env::temp_dir().join(&db_name);
    let storage = Storage::new(
        &StorageConfig::Local { root: dir.clone() },
        "http://api.test",
    )
    .unwrap();
    let state = AppState {
        db,
        storage,
        owner_token: Arc::from(TOKEN),
        media_base_url: Arc::from("http://api.test/media"),
        inline_ingest: true,
    };
    Some(Env {
        app: app(state),
        admin_url,
        db_name,
        dir: TempDir(dir),
    })
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Body>,
    auth: bool,
) -> (StatusCode, Value, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    if auth {
        req = req.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
    }
    let req = match body {
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(b),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, bytes)
}

async fn post(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let (s, v, _) = call(app, "POST", uri, Some(Body::from(body.to_string())), true).await;
    (s, v)
}

// ---------- synthetic Fujifilm JPEGs ----------

/// Little-endian TIFF writer: enough for IFD0, Exif, GPS and a Fujifilm maker note.
struct Ifd(Vec<(u16, u16, u32, Vec<u8>)>);
impl Ifd {
    fn ascii(&mut self, tag: u16, s: &str) -> &mut Self {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        self.0.push((tag, 2, v.len() as u32, v));
        self
    }
    fn short(&mut self, tag: u16, v: u16) -> &mut Self {
        self.0.push((tag, 3, 1, v.to_le_bytes().to_vec()));
        self
    }
    fn long(&mut self, tag: u16, v: u32) -> &mut Self {
        self.0.push((tag, 4, 1, v.to_le_bytes().to_vec()));
        self
    }
    fn rationals(&mut self, tag: u16, v: &[(u32, u32)]) -> &mut Self {
        let b = v
            .iter()
            .flat_map(|(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat())
            .collect();
        self.0.push((tag, 5, v.len() as u32, b));
        self
    }
    fn undefined(&mut self, tag: u16, v: Vec<u8>) -> &mut Self {
        self.0.push((tag, 7, v.len() as u32, v));
        self
    }
    /// Serialises the IFD at absolute position `at`; offsets are relative to `base`.
    fn write(&self, at: usize, base: usize) -> Vec<u8> {
        let mut entries = self.0.clone();
        entries.sort_by_key(|e| e.0);
        let table = 2 + entries.len() * 12 + 4;
        let mut head = (entries.len() as u16).to_le_bytes().to_vec();
        let mut data = Vec::new();
        for (tag, typ, count, val) in &entries {
            head.extend_from_slice(&tag.to_le_bytes());
            head.extend_from_slice(&typ.to_le_bytes());
            head.extend_from_slice(&count.to_le_bytes());
            if val.len() <= 4 {
                let mut v = val.clone();
                v.resize(4, 0);
                head.extend_from_slice(&v);
            } else {
                let off = (at + table + data.len() - base) as u32;
                head.extend_from_slice(&off.to_le_bytes());
                data.extend_from_slice(val);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
            }
        }
        head.extend_from_slice(&0u32.to_le_bytes());
        head.extend(data);
        head
    }
}

#[derive(Clone, Copy)]
struct Shot {
    lat: Option<f64>,
    lon: f64,
    /// "YYYY:MM:DD HH:MM:SS", camera local time at UTC-4.
    when: &'static str,
    film_mode: u16,
}

fn dms(v: f64) -> [(u32, u32); 3] {
    let v = v.abs();
    let d = v.floor();
    let m = ((v - d) * 60.0).floor();
    let s = ((v - d) * 60.0 - m) * 60.0;
    [
        (d as u32, 1),
        (m as u32, 1),
        ((s * 10_000.0).round() as u32, 10_000),
    ]
}

fn fuji_jpeg(shot: Shot) -> Vec<u8> {
    // Maker note: "FUJIFILM" + IFD offset 12, offsets relative to its start.
    let mut mn_ifd = Ifd(Vec::new());
    mn_ifd
        .short(0x1401, shot.film_mode)
        .short(0x1003, 0)
        .long(0x1047, 32);
    let mut makernote = b"FUJIFILM".to_vec();
    makernote.extend_from_slice(&12u32.to_le_bytes());
    let mn_body = mn_ifd.write(12, 0);
    makernote.extend(mn_body);

    let mut exif = Ifd(Vec::new());
    exif.ascii(0x9003, shot.when)
        .ascii(0x9011, "-04:00")
        .rationals(0x829d, &[(56, 10)])
        .rationals(0x829a, &[(1, 250)])
        .short(0x8827, 160);
    exif.undefined(0x927c, makernote);
    let mut gps = Ifd(Vec::new());
    if let Some(lat) = shot.lat {
        gps.ascii(1, if lat < 0.0 { "S" } else { "N" })
            .rationals(2, &dms(lat));
        gps.ascii(3, if shot.lon < 0.0 { "W" } else { "E" })
            .rationals(4, &dms(shot.lon));
    }

    // Layout: header(8) | IFD0 | Exif IFD | GPS IFD
    let mut ifd0 = Ifd(Vec::new());
    ifd0.ascii(0x010f, "FUJIFILM")
        .ascii(0x0110, "X-T5")
        .long(0x8769, 0);
    if shot.lat.is_some() {
        ifd0.long(0x8825, 0);
    }
    let ifd0_len = ifd0.write(8, 0).len();
    let exif_at = 8 + ifd0_len;
    let exif_bytes = exif.write(exif_at, 0);
    let gps_at = exif_at + exif_bytes.len();
    ifd0.0.retain(|e| e.0 != 0x8769 && e.0 != 0x8825);
    ifd0.long(0x8769, exif_at as u32);
    if shot.lat.is_some() {
        ifd0.long(0x8825, gps_at as u32);
    }
    let mut tiff = b"II*\0".to_vec();
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend(ifd0.write(8, 0));
    tiff.extend(exif_bytes);
    if shot.lat.is_some() {
        tiff.extend(gps.write(gps_at, 0));
    }

    let mut img = Vec::new();
    let pixels = image::RgbImage::from_fn(96, 64, |x, y| {
        image::Rgb([(x * 2) as u8, (y * 3) as u8, 120])
    });
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut img, 80)
        .encode_image(&pixels)
        .unwrap();
    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend(tiff);
    let mut out = img[..2].to_vec();
    out.extend(app1);
    out.extend_from_slice(&img[2..]);
    out
}

fn fake_raf(jpeg: &[u8]) -> Vec<u8> {
    let mut raf = vec![0u8; 0x94];
    raf[..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
    let off = raf.len() as u32;
    raf[84..88].copy_from_slice(&off.to_be_bytes());
    raf[88..92].copy_from_slice(&(jpeg.len() as u32).to_be_bytes());
    raf.extend_from_slice(jpeg);
    raf.resize(raf.len() + 2 * 1024 * 1024, 0x55);
    raf
}

async fn upload(app: &Router, trip: &str, name: &str, data: Vec<u8>) -> String {
    let (s, v) = post(
        app,
        &format!("/trips/{trip}/uploads"),
        json!({ "file_name": name, "byte_size": data.len() }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["photo_id"].as_str().unwrap().to_string();
    assert_eq!(v["upload"]["requires_owner_token"], true);
    let (s, _, _) = call(
        app,
        "PUT",
        &format!("/photos/{id}/content"),
        Some(Body::from(data)),
        true,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, v) = post(app, &format!("/photos/{id}/complete"), json!({})).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    id
}

const MONTMORENCY: (f64, f64) = (46.8906, -71.1478);
const VIEUX_QUEBEC: (f64, f64) = (46.8123, -71.2075);
const TADOUSSAC: (f64, f64) = (48.1430, -69.7160);

fn at(p: (f64, f64), dlat: f64, when: &'static str, film_mode: u16) -> Shot {
    Shot {
        lat: Some(p.0 + dlat),
        lon: p.1 - dlat,
        when,
        film_mode,
    }
}

// ---------- tests ----------

#[tokio::test]
async fn health_is_public_and_the_rest_needs_the_token() {
    let Some(env) = setup().await else { return };
    let (s, v, _) = call(&env.app, "GET", "/health", None, false).await;
    assert_eq!((s, v["ok"].clone()), (StatusCode::OK, json!(true)));
    let (s, v, _) = call(&env.app, "GET", "/trips", None, false).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(v["error"], "unauthorized");
    let req = Request::get("/trips")
        .header(header::AUTHORIZATION, "Bearer wrong-token")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        env.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_trip_becomes_stops_route_and_days() {
    let Some(env) = setup().await else { return };
    let app = &env.app;
    let (s, v) = post(
        app,
        "/trips",
        json!({ "slug": "quebec-2026", "title": "L'été indien" }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");

    let shots = [
        // Day 1: Vieux-Québec, evening.
        at(VIEUX_QUEBEC, 0.000, "2026:09:14 18:00:00", 0x600),
        at(VIEUX_QUEBEC, 0.002, "2026:09:14 18:20:00", 0x600),
        // No GPS, 40 minutes later: should join the Vieux-Québec stop by time.
        Shot {
            lat: None,
            lon: 0.0,
            when: "2026:09:14 19:00:00",
            film_mode: 0x600,
        },
        // Day 2: Montmorency falls, 3 photos.
        at(MONTMORENCY, 0.000, "2026:09:15 14:00:00", 0x600),
        at(MONTMORENCY, 0.001, "2026:09:15 14:10:00", 0x700),
        at(MONTMORENCY, 0.003, "2026:09:15 14:30:00", 0x600),
        // Day 4: Tadoussac, 2 photos.
        at(TADOUSSAC, 0.000, "2026:09:17 07:10:00", 0x700),
        at(TADOUSSAC, 0.001, "2026:09:17 08:00:00", 0x700),
        // Day 5: back in Vieux-Québec: a new stop, not the first one.
        at(VIEUX_QUEBEC, 0.001, "2026:09:18 12:00:00", 0xa00),
    ];
    for (i, shot) in shots.iter().enumerate() {
        upload(
            app,
            "quebec-2026",
            &format!("DSCF{:04}.JPG", 2100 + i),
            fuji_jpeg(*shot),
        )
        .await;
    }

    let (s, trip, _) = call(app, "GET", "/trips/quebec-2026", None, true).await;
    assert_eq!(s, StatusCode::OK, "{trip}");
    assert_eq!(
        trip["photos"],
        json!({ "ready": 9, "in_flight": 0, "failed": 0, "with_gps": 8 })
    );

    let stops = trip["stops"].as_array().unwrap();
    assert_eq!(stops.len(), 4, "{stops:#?}");
    let near = |s: &Value, p: (f64, f64)| {
        (s["lat"].as_f64().unwrap() - p.0).abs() < 0.01
            && (s["lon"].as_f64().unwrap() - p.1).abs() < 0.01
    };
    assert!(
        near(&stops[0], VIEUX_QUEBEC)
            && near(&stops[1], MONTMORENCY)
            && near(&stops[2], TADOUSSAC)
            && near(&stops[3], VIEUX_QUEBEC)
    );
    assert_eq!(
        stops[0]["photos"], 3,
        "the photo without GPS joins the stop closest in time"
    );
    assert_eq!(stops[0]["photos_with_gps"], 2);
    assert_eq!(stops[1]["film_simulation"], "Classic Chrome");
    assert_eq!(stops[2]["film_simulation"], "Eterna");
    assert_eq!(
        stops[3]["is_waypoint"], true,
        "a single photo is a waypoint"
    );
    assert_eq!(
        stops[1]["arrived_at"], "2026-09-15T18:00:00Z",
        "local 14:00 at UTC-4"
    );

    let days: Vec<i64> = trip["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["chant"].as_i64().unwrap())
        .collect();
    assert_eq!(
        days,
        vec![1, 2, 4, 5],
        "chants count calendar days from the first photo"
    );
    assert_eq!(
        trip["film_simulations"][0],
        json!({ "name": "Classic Chrome", "photos": 5 })
    );
    let km = trip["route_km"].as_f64().unwrap();
    assert!(
        (330.0..400.0).contains(&km),
        "Québec → Montmorency → Tadoussac → Québec is ~360 km, got {km}"
    );

    let (_, route, _) = call(app, "GET", "/trips/quebec-2026/route", None, true).await;
    assert_eq!(
        route["geometry"]["coordinates"].as_array().unwrap().len(),
        4
    );

    let (_, photos, _) = call(app, "GET", "/trips/quebec-2026/photos", None, true).await;
    let features = photos["features"].as_array().unwrap();
    assert_eq!(features.len(), 9);
    let first = &features[0]["properties"];
    assert_eq!(first["film_simulation"], "Classic Chrome");
    assert_eq!(first["exposure_time"], "1/250");
    assert_eq!(first["recipe"]["grain_roughness"], "Weak");
    assert_eq!(first["captured_local"], "2026-09-14T18:00:00");
    assert!(first["palette"].as_array().is_some_and(|p| !p.is_empty()));
    assert_eq!(
        features.iter().filter(|f| f["geometry"].is_null()).count(),
        1
    );

    // Thumbnails are served; originals are not.
    let thumb = first["thumb_url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://api.test")
        .unwrap()
        .to_string();
    let (s, _, bytes) = call(app, "GET", &thumb, None, false).await;
    assert_eq!(s, StatusCode::OK);
    assert!(bytes.starts_with(&[0xFF, 0xD8]));
    let original = thumb.replace("thumb.jpg", "original.jpg");
    assert_eq!(
        call(app, "GET", &original, None, false).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn stop_names_survive_recomputation() {
    let Some(env) = setup().await else { return };
    let app = &env.app;
    post(app, "/trips", json!({ "slug": "q", "title": "Q" })).await;
    for (i, when) in [
        "2026:09:15 14:00:00",
        "2026:09:15 14:10:00",
        "2026:09:15 14:20:00",
    ]
    .iter()
    .enumerate()
    {
        upload(
            app,
            "q",
            &format!("{i}.jpg"),
            fuji_jpeg(at(MONTMORENCY, 0.001 * i as f64, when, 0x600)),
        )
        .await;
    }
    let (_, trip, _) = call(app, "GET", "/trips/q", None, true).await;
    let id = trip["stops"][0]["id"].as_str().unwrap().to_string();
    let (s, _, _) = call(
        app,
        "PATCH",
        &format!("/stops/{id}"),
        Some(Body::from(
            json!({ "name": "Chutes Montmorency" }).to_string(),
        )),
        true,
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // A new photo triggers a recomputation; the name must stay.
    upload(
        app,
        "q",
        "3.jpg",
        fuji_jpeg(at(MONTMORENCY, 0.002, "2026:09:15 14:40:00", 0x600)),
    )
    .await;
    let (_, trip, _) = call(app, "GET", "/trips/q", None, true).await;
    assert_eq!(trip["stops"][0]["name"], "Chutes Montmorency");
    assert_eq!(trip["stops"][0]["photos"], 4);
}

fn contains_file(dir: &std::path::Path, name: &str) -> bool {
    std::fs::read_dir(dir).is_ok_and(|it| {
        it.flatten().any(|e| {
            if e.path().is_dir() {
                contains_file(&e.path(), name)
            } else {
                e.file_name().to_string_lossy() == name
            }
        })
    })
}

#[tokio::test]
async fn originals_stay_private_and_files_are_listed() {
    let Some(stack) = setup().await else { return };
    let app = &stack.app;
    post(app, "/trips", json!({ "slug": "p", "title": "P" })).await;
    let id = upload(
        app,
        "p",
        "DSCF0001.JPG",
        fuji_jpeg(at(MONTMORENCY, 0.0, "2026:09:15 14:00:00", 0x600)),
    )
    .await;

    // The original sits in the private store only; the media store holds derivatives.
    let root = &stack.dir.0;
    assert!(contains_file(&root.join("originals"), "original.jpg"));
    assert!(
        !contains_file(&root.join("media"), "original.jpg"),
        "an original leaked into the public media store"
    );
    assert!(
        contains_file(&root.join("media"), "thumb.jpg")
            && contains_file(&root.join("media"), "preview.jpg")
    );
    assert!(!contains_file(&root.join("originals"), "thumb.jpg"));

    let (s, files, _) = call(app, "GET", "/trips/p/files", None, true).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(files[0]["id"], json!(id));
    assert_eq!(files[0]["file_name"], "DSCF0001.JPG");
    assert_eq!(files[0]["status"], "ready");
    assert!(files[0]["byte_size"].as_i64().unwrap() > 1000);
    assert_eq!(files.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn raf_is_ingested_from_its_preview() {
    let Some(env) = setup().await else { return };
    let app = &env.app;
    post(app, "/trips", json!({ "slug": "raf", "title": "RAF" })).await;
    let id = upload(
        app,
        "raf",
        "DSCF0001.RAF",
        fake_raf(&fuji_jpeg(at(TADOUSSAC, 0.0, "2026:09:17 07:10:00", 0xb00))),
    )
    .await;
    let (_, p, _) = call(app, "GET", &format!("/photos/{id}"), None, true).await;
    assert_eq!(p["status"], "ready", "{p}");
    assert_eq!(p["container"], "raf");
    assert_eq!(p["film_simulation"], "Reala Ace");
    assert_eq!(
        (p["width"].clone(), p["height"].clone()),
        (json!(96), json!(64))
    );
}

#[tokio::test]
async fn bad_requests_are_explained() {
    let Some(env) = setup().await else { return };
    let app = &env.app;
    let (s, v) = post(app, "/trips", json!({ "slug": "Bad Slug", "title": "x" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["message"].as_str().unwrap().contains("slug"));
    post(app, "/trips", json!({ "slug": "t", "title": "T" })).await;
    assert_eq!(
        post(app, "/trips", json!({ "slug": "t", "title": "T" }))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        post(
            app,
            "/trips/nope/uploads",
            json!({ "file_name": "a.jpg", "byte_size": 10 })
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let (s, v) = post(
        app,
        "/trips/t/uploads",
        json!({ "file_name": "notes.png", "byte_size": 10 }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["message"].as_str().unwrap().contains(".raf"));
    assert_eq!(
        post(
            app,
            "/trips/t/uploads",
            json!({ "file_name": "a.jpg", "byte_size": 0 })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let (_, v) = post(
        app,
        "/trips/t/uploads",
        json!({ "file_name": "a.jpg", "byte_size": 5 }),
    )
    .await;
    let id = v["photo_id"].as_str().unwrap();
    // Completing before uploading, and uploading the wrong size, are refused.
    assert_eq!(
        post(app, &format!("/photos/{id}/complete"), json!({}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (s, _, _) = call(
        app,
        "PUT",
        &format!("/photos/{id}/content"),
        Some(Body::from(vec![1u8; 4])),
        true,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A file that is not a photo is stored but marked failed with a reason.
    call(
        app,
        "PUT",
        &format!("/photos/{id}/content"),
        Some(Body::from(b"hello".to_vec())),
        true,
    )
    .await;
    post(app, &format!("/photos/{id}/complete"), json!({})).await;
    let (_, p, _) = call(app, "GET", &format!("/photos/{id}"), None, true).await;
    assert_eq!(p["status"], "failed");
    assert!(p["error"].as_str().unwrap().starts_with("metadata:"), "{p}");
}
