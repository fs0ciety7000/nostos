//! `nostos-scan <folder> [-o out.geojson]`
//!
//! Walks a folder of JPEG/RAF files, reads their metadata with nostos-exif
//! and writes a `GeoJSON` `FeatureCollection` sorted by capture time. Photos
//! without GPS are kept with a `null` geometry so they can be placed later
//! (GPX matching or by hand). A summary goes to stderr.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nostos_exif::Metadata;
use serde_json::{json, Value};

const USAGE: &str = "usage: nostos-scan <folder> [-o out.geojson]

Reads GPS, capture time and Fujifilm film-simulation settings from every
.jpg/.jpeg/.raf file under <folder> and prints GeoJSON (or writes it to -o).";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut folder = None;
    let mut out = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-o" | "--output" => out = args.next().map(PathBuf::from),
            _ if folder.is_none() => folder = Some(PathBuf::from(a)),
            _ => {
                eprintln!("unexpected argument: {a}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(folder) = folder else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if !folder.is_dir() {
        eprintln!("{} is not a folder", folder.display());
        return ExitCode::from(2);
    }

    let mut files = Vec::new();
    walk(&folder, &mut files);
    files.sort();

    let mut photos = Vec::new();
    let mut failed = Vec::new();
    for path in &files {
        match nostos_exif::read_path(path) {
            Ok(m) => photos.push((path.clone(), m)),
            Err(e) => failed.push((path.clone(), e)),
        }
    }
    // Chronological order is the itinerary order; undated photos go last.
    photos.sort_by_key(|a| (a.1.captured_at.is_none(), a.1.captured_at));

    let features: Vec<Value> = photos.iter().map(|(p, m)| feature(&folder, p, m)).collect();
    let doc = json!({ "type": "FeatureCollection", "features": features });
    let text = serde_json::to_string_pretty(&doc).expect("serializable");

    let written = match &out {
        Some(path) => std::fs::write(path, text + "\n"),
        None => writeln!(std::io::stdout().lock(), "{text}"),
    };
    if let Err(e) = written {
        eprintln!("could not write output: {e}");
        return ExitCode::FAILURE;
    }
    summary(files.len(), &photos, &failed, out.as_deref());
    ExitCode::SUCCESS
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "raf"))
        {
            out.push(p);
        }
    }
}

fn feature(root: &Path, path: &Path, m: &Metadata) -> Value {
    let geometry = m.gps.map_or(Value::Null, |g| {
        let mut coords = vec![json!(round(g.longitude, 7)), json!(round(g.latitude, 7))];
        if let Some(alt) = g.altitude_m {
            coords.push(json!(round(alt, 1)));
        }
        json!({ "type": "Point", "coordinates": coords })
    });
    let e = &m.exposure;
    let fuji = m.fuji.as_ref();
    json!({
        "type": "Feature",
        "geometry": geometry,
        "properties": {
            "file": path.strip_prefix(root).unwrap_or(path).to_string_lossy(),
            "container": m.container,
            "camera": m.model,
            "lens": m.lens_model,
            "captured_at": m.captured_at.map(|d| d.to_iso(m.utc_offset_minutes)),
            "gps_time_utc": m.gps.and_then(|g| g.timestamp_utc).map(|t| t.to_iso(Some(0))),
            "f_number": e.f_number,
            "exposure_time": e.exposure_time.map(|r| if r.num == 1 || r.num >= r.den { format!("{}/{}", r.num, r.den) } else { format!("1/{}", (r.den as f64 / r.num as f64).round()) }),
            "iso": e.iso,
            "focal_length_mm": e.focal_length_mm,
            "focal_length_35mm": e.focal_length_35mm,
            "film_simulation": fuji.and_then(|f| f.film_simulation).map(|f| f.name()),
            "recipe": fuji,
        }
    })
}

fn round(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}

fn summary(
    found: usize,
    photos: &[(PathBuf, Metadata)],
    failed: &[(PathBuf, nostos_exif::Error)],
    out: Option<&Path>,
) {
    let with_gps = photos.iter().filter(|(_, m)| m.gps.is_some()).count();
    let mut films: BTreeMap<String, usize> = BTreeMap::new();
    for (_, m) in photos {
        let name = m
            .fuji
            .as_ref()
            .and_then(|f| f.film_simulation)
            .map_or_else(|| "(no film simulation)".to_string(), |f| f.name());
        *films.entry(name).or_default() += 1;
    }
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "\n{found} files · {} read · {with_gps} with GPS · {} without",
        photos.len(),
        photos.len() - with_gps
    );
    let mut films: Vec<_> = films.into_iter().collect();
    films.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (name, n) in films {
        let _ = writeln!(err, "  {n:>5}  {name}");
    }
    for (p, e) in failed {
        let _ = writeln!(err, "  skipped {}: {e}", p.display());
    }
    if let Some(out) = out {
        let _ = writeln!(err, "GeoJSON written to {}", out.display());
    }
}
