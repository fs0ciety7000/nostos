//! Compares nostos-exif against exiftool on every fixture.
//!
//! Each `tests/fixtures/<name>.jpg` carries the Exif block of a real
//! Fujifilm file (or one exiftool wrote recipe settings into), and
//! `<name>.json` is exiftool's numeric (`-n`) reading of it. See
//! `scripts/make-fixture.py` and `scripts/make-recipe-fixtures.sh`.

use std::path::{Path, PathBuf};

use nostos_exif::{
    DRangePriority, DRangeStrength, DynamicRange, FilmSimulation, GrainSize, Metadata, MonoFilter,
    Strength, WhiteBalance,
};
use serde_json::Value;

fn fixtures() -> Vec<(String, PathBuf, Value)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures dir")
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "json").then_some(p)
        })
        .map(|json| {
            let name = json.file_stem().unwrap().to_string_lossy().to_string();
            let golden: Value =
                serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
            (name, json.with_extension("jpg"), golden)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(
        out.len() >= 14,
        "expected the full fixture set, found {}",
        out.len()
    );
    out
}

fn num(g: &Value, key: &str) -> Option<f64> {
    match g.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}
fn int(g: &Value, key: &str) -> Option<i64> {
    num(g, key).map(|v| v as i64)
}
fn text(g: &Value, key: &str) -> Option<String> {
    g.get(key).map(|v| match v {
        // nostos-exif trims padding spaces that cameras leave in ASCII tags.
        Value::String(s) => s.trim().to_string(),
        other => other.to_string(),
    })
}
fn close(a: Option<f64>, b: Option<f64>, eps: f64) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= eps * b.abs().max(1.0),
        (None, None) => true,
        _ => false,
    }
}

fn strength(code: Option<i64>) -> Option<Strength> {
    match code? {
        0 => Some(Strength::Off),
        32 => Some(Strength::Weak),
        64 => Some(Strength::Strong),
        _ => None,
    }
}

#[allow(clippy::too_many_lines)] // one flat list of field comparisons reads best
fn check(name: &str, m: &Metadata, g: &Value) -> Vec<String> {
    let mut errs = Vec::new();
    let mut eq = |field: &str, ok: bool, ours: String, theirs: String| {
        if !ok {
            errs.push(format!("{name}: {field}: ours={ours} exiftool={theirs}"));
        }
    };

    // --- IFD0 / Exif ---
    eq(
        "make",
        m.make == text(g, "IFD0:Make"),
        format!("{:?}", m.make),
        format!("{:?}", text(g, "IFD0:Make")),
    );
    eq(
        "model",
        m.model == text(g, "IFD0:Model"),
        format!("{:?}", m.model),
        format!("{:?}", text(g, "IFD0:Model")),
    );
    eq(
        "lens",
        m.lens_model == text(g, "ExifIFD:LensModel"),
        format!("{:?}", m.lens_model),
        format!("{:?}", text(g, "ExifIFD:LensModel")),
    );
    let date = m.captured_at.map(|d| {
        format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
            d.year, d.month, d.day, d.hour, d.minute, d.second
        )
    });
    eq(
        "date",
        date == text(g, "ExifIFD:DateTimeOriginal"),
        format!("{date:?}"),
        format!("{:?}", text(g, "ExifIFD:DateTimeOriginal")),
    );
    let offset = text(g, "ExifIFD:OffsetTimeOriginal");
    let ours_offset = m.captured_at.map(|d| d.to_iso(m.utc_offset_minutes));
    eq(
        "utc offset",
        offset
            .as_deref()
            .map_or(m.utc_offset_minutes.is_none(), |o| {
                ours_offset
                    .as_deref()
                    .is_some_and(|s| s.ends_with(o) || (o == "+00:00" && s.ends_with('Z')))
            }),
        format!("{ours_offset:?}"),
        format!("{offset:?}"),
    );
    let e = &m.exposure;
    eq(
        "f-number",
        close(e.f_number, num(g, "ExifIFD:FNumber"), 1e-6),
        format!("{:?}", e.f_number),
        format!("{:?}", num(g, "ExifIFD:FNumber")),
    );
    eq(
        "exposure",
        close(
            e.exposure_time.and_then(nostos_exif::Rational::to_f64),
            num(g, "ExifIFD:ExposureTime"),
            1e-6,
        ),
        format!("{:?}", e.exposure_time),
        format!("{:?}", num(g, "ExifIFD:ExposureTime")),
    );
    eq(
        "iso",
        e.iso.map(i64::from) == int(g, "ExifIFD:ISO"),
        format!("{:?}", e.iso),
        format!("{:?}", int(g, "ExifIFD:ISO")),
    );
    eq(
        "focal",
        close(e.focal_length_mm, num(g, "ExifIFD:FocalLength"), 1e-6),
        format!("{:?}", e.focal_length_mm),
        format!("{:?}", num(g, "ExifIFD:FocalLength")),
    );
    eq(
        "focal 35",
        e.focal_length_35mm.map(i64::from) == int(g, "ExifIFD:FocalLengthIn35mmFormat"),
        format!("{:?}", e.focal_length_35mm),
        format!("{:?}", int(g, "ExifIFD:FocalLengthIn35mmFormat")),
    );
    eq(
        "width",
        m.width.map(i64::from) == int(g, "ExifIFD:ExifImageWidth"),
        format!("{:?}", m.width),
        format!("{:?}", int(g, "ExifIFD:ExifImageWidth")),
    );

    // --- GPS (Composite values are signed) ---
    let lat = m.gps.map(|p| p.latitude);
    let lon = m.gps.map(|p| p.longitude);
    eq(
        "latitude",
        close(lat, num(g, "Composite:GPSLatitude"), 1e-9),
        format!("{lat:?}"),
        format!("{:?}", num(g, "Composite:GPSLatitude")),
    );
    eq(
        "longitude",
        close(lon, num(g, "Composite:GPSLongitude"), 1e-9),
        format!("{lon:?}"),
        format!("{:?}", num(g, "Composite:GPSLongitude")),
    );
    let alt = m.gps.and_then(|p| p.altitude_m);
    eq(
        "altitude",
        close(alt, num(g, "Composite:GPSAltitude"), 1e-6),
        format!("{alt:?}"),
        format!("{:?}", num(g, "Composite:GPSAltitude")),
    );

    // --- Fujifilm maker note ---
    let Some(f) = &m.fuji else {
        errs.push(format!("{name}: no Fujifilm maker note parsed"));
        return errs;
    };
    let film_mode = int(g, "FujiFilm:FilmMode").map(|v| v as u16);
    let saturation = int(g, "FujiFilm:Saturation").map(|v| v as u16);
    let expected_film = FilmSimulation::from_codes(film_mode, saturation);
    eq(
        "film simulation",
        f.film_simulation == expected_film,
        format!("{:?}", f.film_simulation),
        format!("{expected_film:?} (FilmMode {film_mode:?}, Saturation {saturation:?})"),
    );
    eq(
        "grain roughness",
        f.grain_roughness == strength(int(g, "FujiFilm:GrainEffectRoughness")),
        format!("{:?}", f.grain_roughness),
        format!("{:?}", int(g, "FujiFilm:GrainEffectRoughness")),
    );
    let size = match int(g, "FujiFilm:GrainEffectSize") {
        Some(0) => Some(GrainSize::Off),
        Some(16) => Some(GrainSize::Small),
        Some(32) => Some(GrainSize::Large),
        _ => None,
    };
    eq(
        "grain size",
        f.grain_size == size,
        format!("{:?}", f.grain_size),
        format!("{size:?}"),
    );
    eq(
        "color chrome",
        f.color_chrome == strength(int(g, "FujiFilm:ColorChromeEffect")),
        format!("{:?}", f.color_chrome),
        format!("{:?}", int(g, "FujiFilm:ColorChromeEffect")),
    );
    eq(
        "cc fx blue",
        f.color_chrome_fx_blue == strength(int(g, "FujiFilm:ColorChromeFXBlue")),
        format!("{:?}", f.color_chrome_fx_blue),
        format!("{:?}", int(g, "FujiFilm:ColorChromeFXBlue")),
    );
    let tone = |k| num(g, k).map(|v| -v / 16.0);
    eq(
        "shadow tone",
        close(
            f.shadow_tone.map(f64::from),
            tone("FujiFilm:ShadowTone"),
            1e-9,
        ),
        format!("{:?}", f.shadow_tone),
        format!("{:?}", tone("FujiFilm:ShadowTone")),
    );
    eq(
        "highlight tone",
        close(
            f.highlight_tone.map(f64::from),
            tone("FujiFilm:HighlightTone"),
            1e-9,
        ),
        format!("{:?}", f.highlight_tone),
        format!("{:?}", tone("FujiFilm:HighlightTone")),
    );
    let clarity = int(g, "FujiFilm:Clarity").map(|v| (v / 1000) as i8);
    eq(
        "clarity",
        f.clarity == clarity,
        format!("{:?}", f.clarity),
        format!("{clarity:?}"),
    );
    let wb = text(g, "FujiFilm:WhiteBalanceFineTune").map(|s| {
        s.split_whitespace()
            .map(|v| v.parse::<i32>().unwrap())
            .collect::<Vec<_>>()
    });
    let ours_wb = f.wb_shift.map(|w| vec![w.red_raw, w.blue_raw]);
    eq(
        "wb fine tune",
        ours_wb == wb,
        format!("{ours_wb:?}"),
        format!("{wb:?}"),
    );
    let dr = match int(g, "FujiFilm:DynamicRangeSetting") {
        Some(0) => Some(DynamicRange::Auto),
        Some(1) => {
            int(g, "FujiFilm:DevelopmentDynamicRange").map(|v| DynamicRange::Percent(v as u16))
        }
        Some(0x100) => Some(DynamicRange::Percent(100)),
        Some(0x200) => Some(DynamicRange::Percent(200)),
        Some(0x201) => Some(DynamicRange::Percent(400)),
        _ => None,
    };
    eq(
        "dynamic range",
        f.dynamic_range == dr,
        format!("{:?}", f.dynamic_range),
        format!("{dr:?}"),
    );
    let wb_mode = int(g, "FujiFilm:WhiteBalance").map(|v| v as u16);
    eq(
        "white balance present",
        f.white_balance.is_some() == wb_mode.is_some(),
        format!("{:?}", f.white_balance),
        format!("{wb_mode:?}"),
    );
    let drp = match int(g, "FujiFilm:DRangePriority") {
        Some(0) => Some(DRangePriority::Auto(
            match int(g, "FujiFilm:DRangePriorityAuto") {
                Some(1) => Some(DRangeStrength::Weak),
                Some(2) => Some(DRangeStrength::Strong),
                Some(3) => Some(DRangeStrength::Plus),
                _ => None,
            },
        )),
        Some(1) => match int(g, "FujiFilm:DRangePriorityFixed") {
            Some(1) => Some(DRangePriority::Fixed(DRangeStrength::Weak)),
            Some(2) => Some(DRangePriority::Fixed(DRangeStrength::Strong)),
            _ => None,
        },
        _ => None,
    };
    eq(
        "d-range priority",
        f.d_range_priority == drp,
        format!("{:?}", f.d_range_priority),
        format!("{drp:?}"),
    );
    errs
}

#[test]
fn matches_exiftool_on_every_fixture() {
    let mut errs = Vec::new();
    for (name, jpg, golden) in fixtures() {
        let bytes = std::fs::read(&jpg).unwrap();
        match nostos_exif::read(&bytes) {
            Ok(m) => errs.extend(check(&name, &m, &golden)),
            Err(e) => errs.push(format!("{name}: read failed: {e}")),
        }
        // The streaming reader must agree with the in-memory one.
        let streamed = nostos_exif::read_path(&jpg);
        assert_eq!(
            streamed,
            nostos_exif::read(&bytes),
            "{name}: read_path differs from read"
        );
    }
    assert!(
        errs.is_empty(),
        "{} mismatch(es):\n{}",
        errs.len(),
        errs.join("\n")
    );
}

#[test]
fn recipe_values_read_as_the_camera_menu_shows_them() {
    let read = |n: &str| {
        nostos_exif::read_path(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{n}.jpg")),
        )
        .unwrap()
        .fuji
        .unwrap()
    };

    let cc = read("recipe-classic-chrome");
    assert_eq!(
        cc.film_simulation.map(|f| f.name()),
        Some("Classic Chrome".into())
    );
    assert_eq!(cc.color, Some(1));
    assert_eq!((cc.shadow_tone, cc.highlight_tone), (Some(1.0), Some(-1.0)));
    assert_eq!(
        (cc.grain_roughness, cc.grain_size),
        (Some(Strength::Weak), Some(GrainSize::Small))
    );
    assert_eq!(
        (cc.color_chrome, cc.color_chrome_fx_blue),
        (Some(Strength::Strong), Some(Strength::Weak))
    );
    assert_eq!(cc.wb_shift.and_then(|w| w.steps()), Some((2, -4)));
    assert_eq!(cc.dynamic_range, Some(DynamicRange::Percent(400)));
    assert_eq!(
        (cc.clarity, cc.noise_reduction, cc.sharpness),
        (Some(-2), Some(-1), Some(-1))
    );

    let acros = read("recipe-acros-red");
    assert_eq!(
        acros.film_simulation,
        Some(FilmSimulation::Acros(MonoFilter::Red))
    );
    assert_eq!(
        acros.film_simulation.map(|f| f.name()),
        Some("Acros+R".into())
    );
    assert_eq!(acros.color, None, "monochrome has no Color setting");
    assert_eq!(
        (acros.shadow_tone, acros.highlight_tone),
        (Some(2.0), Some(0.5))
    );
    assert_eq!(acros.dynamic_range, Some(DynamicRange::Auto));

    let nn = read("recipe-nostalgic-kelvin");
    assert_eq!(nn.film_simulation, Some(FilmSimulation::NostalgicNeg));
    assert_eq!(nn.color, Some(-3));
    assert_eq!((nn.shadow_tone, nn.highlight_tone), (Some(-0.5), Some(3.0)));
    assert_eq!(nn.white_balance, Some(WhiteBalance::Kelvin));
    assert_eq!(nn.wb_shift.and_then(|w| w.steps()), Some((-1, 5)));
}

#[test]
fn gps_position_time_and_offset() {
    let m = nostos_exif::read_path(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gps-montmorency.jpg"),
    )
    .unwrap();
    let gps = m.gps.expect("gps");
    assert!((gps.latitude - 46.890_612).abs() < 1e-9);
    assert!((gps.longitude + 71.147_805).abs() < 1e-9);
    assert_eq!(
        gps.timestamp_utc.map(|t| t.to_iso(Some(0))),
        Some("2026-09-15T18:02:11Z".into())
    );
    assert_eq!(
        m.captured_at.map(|d| d.to_iso(m.utc_offset_minutes)),
        Some("2026-09-15T14:02:11.250-04:00".into())
    );
}
