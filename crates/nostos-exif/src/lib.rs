//! Reads the metadata Nostos needs from Fujifilm JPEG and RAF files:
//! capture time, exposure, GPS position and the in-camera film simulation
//! "recipe" stored in the Fujifilm maker note.
//!
//! - No dependencies (optional `serde` feature), no `unsafe`, no panics on
//!   malformed input: every read is bounds-checked.
//! - [`read_from`] only reads the bytes it needs: the first 256 KiB of a
//!   JPEG, or the RAF header plus the start of its embedded JPEG preview,
//!   never the raw sensor data.
//!
//! ```no_run
//! let meta = nostos_exif::read_path("DSCF0021.RAF")?;
//! if let Some(gps) = meta.gps {
//!     println!("{} {}", gps.latitude, gps.longitude);
//! }
//! if let Some(film) = meta.fuji.and_then(|f| f.film_simulation) {
//!     println!("{}", film.name());
//! }
//! # Ok::<(), nostos_exif::Error>(())
//! ```

mod bytes;
mod container;
mod fuji;
mod tiff;

use std::fmt::{self, Write as _};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub use container::Container;
pub use fuji::{
    DRangePriority, DRangeStrength, DynamicRange, FilmSimulation, FujiSettings, GrainSize,
    MonoFilter, Strength, WbShift, WhiteBalance,
};

use bytes::Bytes;
use tiff::Ifd;

/// Why metadata could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Not a JPEG, RAF or TIFF file.
    UnknownFormat,
    /// A supported container without an Exif block (e.g. a re-exported JPEG).
    NoExif,
    /// The data ends before a structure it announces.
    Truncated,
    /// A structure is internally inconsistent.
    Malformed(&'static str),
    Io(std::io::ErrorKind),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat => f.write_str("not a JPEG, RAF or TIFF file"),
            Self::NoExif => f.write_str("no Exif metadata"),
            Self::Truncated => f.write_str("file is truncated"),
            Self::Malformed(what) => write!(f, "malformed file: {what}"),
            Self::Io(kind) => write!(f, "read error: {kind}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Self::Truncated
        } else {
            Self::Io(e.kind())
        }
    }
}

/// A TIFF rational, kept exact (1/250 s stays 1/250).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Rational {
    pub num: i64,
    pub den: i64,
}

impl Rational {
    pub fn to_f64(self) -> Option<f64> {
        (self.den != 0).then(|| self.num as f64 / self.den as f64)
    }
}

/// Local wall-clock date and time as recorded by the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub millis: Option<u16>,
}

impl DateTime {
    /// Parses the Exif `YYYY:MM:DD HH:MM:SS` form.
    fn parse_exif(s: &str) -> Option<Self> {
        let s = s.as_bytes();
        if s.len() < 19 {
            return None;
        }
        let num = |a: usize, b: usize| {
            std::str::from_utf8(&s[a..b])
                .ok()?
                .trim()
                .parse::<u16>()
                .ok()
        };
        let dt = Self {
            year: num(0, 4)?,
            month: num(5, 7)? as u8,
            day: num(8, 10)? as u8,
            hour: num(11, 13)? as u8,
            minute: num(14, 16)? as u8,
            second: num(17, 19)? as u8,
            millis: None,
        };
        let valid = dt.year > 0
            && (1..=12).contains(&dt.month)
            && (1..=31).contains(&dt.day)
            && dt.hour < 24
            && dt.minute < 60
            && dt.second < 61;
        valid.then_some(dt)
    }

    /// ISO 8601, with the UTC offset when known: `2026-09-15T14:02:11+02:00`.
    pub fn to_iso(&self, offset_minutes: Option<i16>) -> String {
        let mut s = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        );
        if let Some(ms) = self.millis {
            let _ = write!(s, ".{ms:03}");
        }
        match offset_minutes {
            Some(0) => s.push('Z'),
            Some(m) => {
                let _ = write!(
                    s,
                    "{}{:02}:{:02}",
                    if m < 0 { '-' } else { '+' },
                    m.unsigned_abs() / 60,
                    m.unsigned_abs() % 60
                );
            }
            None => {}
        }
        s
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Exposure {
    pub f_number: Option<f64>,
    pub exposure_time: Option<Rational>,
    pub iso: Option<u32>,
    pub focal_length_mm: Option<f64>,
    pub focal_length_35mm: Option<u16>,
    pub exposure_bias_ev: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Gps {
    /// Decimal degrees, south negative.
    pub latitude: f64,
    /// Decimal degrees, west negative.
    pub longitude: f64,
    /// Metres above sea level.
    pub altitude_m: Option<f64>,
    /// UTC time of the GPS fix (smartphone sync sends it with the position).
    pub timestamp_utc: Option<DateTime>,
}

/// Everything Nostos reads from one photo.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Metadata {
    pub container: Container,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens_model: Option<String>,
    pub captured_at: Option<DateTime>,
    /// Offset from UTC of `captured_at`, from `OffsetTimeOriginal`.
    pub utc_offset_minutes: Option<i16>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: Option<u16>,
    pub exposure: Exposure,
    pub gps: Option<Gps>,
    /// Present when the file has a Fujifilm maker note.
    pub fuji: Option<FujiSettings>,
}

mod tag {
    pub const MAKE: u16 = 0x010f;
    pub const MODEL: u16 = 0x0110;
    pub const ORIENTATION: u16 = 0x0112;
    pub const EXIF_IFD: u16 = 0x8769;
    pub const GPS_IFD: u16 = 0x8825;
    pub const EXPOSURE_TIME: u16 = 0x829a;
    pub const F_NUMBER: u16 = 0x829d;
    pub const ISO: u16 = 0x8827;
    pub const DATE_TIME_ORIGINAL: u16 = 0x9003;
    pub const OFFSET_TIME_ORIGINAL: u16 = 0x9011;
    pub const EXPOSURE_BIAS: u16 = 0x9204;
    pub const FOCAL_LENGTH: u16 = 0x920a;
    pub const MAKER_NOTE: u16 = 0x927c;
    pub const SUBSEC_ORIGINAL: u16 = 0x9291;
    pub const PIXEL_X: u16 = 0xa002;
    pub const PIXEL_Y: u16 = 0xa003;
    pub const FOCAL_35MM: u16 = 0xa405;
    pub const LENS_MODEL: u16 = 0xa434;
    pub const GPS_LAT_REF: u16 = 1;
    pub const GPS_LAT: u16 = 2;
    pub const GPS_LON_REF: u16 = 3;
    pub const GPS_LON: u16 = 4;
    pub const GPS_ALT_REF: u16 = 5;
    pub const GPS_ALT: u16 = 6;
    pub const GPS_TIME: u16 = 7;
    pub const GPS_DATE: u16 = 29;
}

/// Reads metadata from a whole file held in memory.
pub fn read(data: &[u8]) -> Result<Metadata, Error> {
    let (container, tiff) = container::find_tiff(data)?;
    parse_tiff(container, tiff)
}

/// Bytes read from the start of a JPEG (or of a RAF's embedded JPEG). The
/// Exif APP1 segment is at most 64 KiB and sits right after SOI (and APP0).
const HEAD: u64 = 256 * 1024;
/// Second, larger attempt for files with big segments before APP1.
const HEAD_RETRY: u64 = 2 * 1024 * 1024;

/// Reads metadata from a seekable source, loading only the bytes it needs.
pub fn read_from<R: Read + Seek>(r: &mut R) -> Result<Metadata, Error> {
    let mut head = [0u8; 160];
    let n = read_up_to(r, &mut head)?;
    let head = &head[..n];
    match container::detect(head)? {
        Container::Tiff => {
            let mut all = Vec::new();
            r.seek(SeekFrom::Start(0))?;
            r.take(HEAD_RETRY * 8).read_to_end(&mut all)?;
            read(&all)
        }
        Container::Jpeg => read_jpeg_at(r, 0, None, Container::Jpeg),
        Container::Raf => {
            let (off, len) = container::raf_jpeg(head)?;
            read_jpeg_at(r, off as u64, Some(len as u64), Container::Raf)
        }
    }
}

/// Opens and reads a file from disk.
pub fn read_path(path: impl AsRef<Path>) -> Result<Metadata, Error> {
    let mut f = std::fs::File::open(path)?;
    read_from(&mut f)
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize, Error> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(n)
}

fn read_jpeg_at<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    len: Option<u64>,
    kind: Container,
) -> Result<Metadata, Error> {
    for budget in [HEAD, HEAD_RETRY] {
        let take = len.map_or(budget, |l| l.min(budget));
        r.seek(SeekFrom::Start(offset))?;
        let mut buf = Vec::with_capacity(take as usize);
        r.by_ref().take(take).read_to_end(&mut buf)?;
        if !buf.starts_with(&[0xFF, 0xD8]) {
            return Err(Error::Malformed("expected a JPEG stream"));
        }
        match container::jpeg_exif(&buf) {
            Ok(tiff) => return parse_tiff(kind, tiff),
            Err(Error::Truncated)
                if (buf.len() as u64) == take && len.map_or(true, |l| l > take) => {}
            Err(e) => return Err(e),
        }
    }
    Err(Error::Truncated)
}

fn parse_tiff(container: Container, tiff: &[u8]) -> Result<Metadata, Error> {
    let (endian, ifd0_off) = tiff::header(tiff)?;
    let b = Bytes::new(tiff, endian);
    let ifd0 = Ifd::read(b, 0, ifd0_off)?;

    let mut meta = Metadata {
        container,
        make: ifd0.ascii(tag::MAKE),
        model: ifd0.ascii(tag::MODEL),
        lens_model: None,
        captured_at: None,
        utc_offset_minutes: None,
        width: None,
        height: None,
        orientation: ifd0
            .int(tag::ORIENTATION)
            .and_then(|v| u16::try_from(v).ok()),
        exposure: Exposure::default(),
        gps: None,
        fuji: None,
    };

    // A broken sub-IFD only loses that part, not the whole file.
    if let Some(exif) = ifd0
        .pointer(tag::EXIF_IFD)
        .and_then(|off| Ifd::read(b, 0, off).ok())
    {
        let pos_f64 = |r: Option<Rational>| {
            r.and_then(Rational::to_f64)
                .filter(|v| v.is_finite() && *v > 0.0)
        };
        meta.exposure = Exposure {
            f_number: pos_f64(exif.rational(tag::F_NUMBER)),
            exposure_time: exif
                .rational(tag::EXPOSURE_TIME)
                .filter(|r| r.den != 0 && r.num > 0),
            iso: exif
                .int(tag::ISO)
                .and_then(|v| u32::try_from(v).ok())
                .filter(|&v| v > 0),
            focal_length_mm: pos_f64(exif.rational(tag::FOCAL_LENGTH)),
            focal_length_35mm: exif
                .int(tag::FOCAL_35MM)
                .and_then(|v| u16::try_from(v).ok())
                .filter(|&v| v > 0),
            exposure_bias_ev: exif.rational(tag::EXPOSURE_BIAS).and_then(Rational::to_f64),
        };
        meta.lens_model = exif.ascii(tag::LENS_MODEL);
        meta.captured_at = exif
            .ascii(tag::DATE_TIME_ORIGINAL)
            .and_then(|s| DateTime::parse_exif(&s))
            .map(|mut dt| {
                dt.millis = exif
                    .ascii(tag::SUBSEC_ORIGINAL)
                    .and_then(|s| subsec_millis(&s));
                dt
            });
        meta.utc_offset_minutes = exif
            .ascii(tag::OFFSET_TIME_ORIGINAL)
            .and_then(|s| parse_offset(&s));
        meta.width = exif.int(tag::PIXEL_X).and_then(|v| u32::try_from(v).ok());
        meta.height = exif.int(tag::PIXEL_Y).and_then(|v| u32::try_from(v).ok());
        if let Some(mn) = exif.entry(tag::MAKER_NOTE) {
            meta.fuji = fuji::parse(tiff, mn.at);
        }
    }

    if let Some(gps) = ifd0
        .pointer(tag::GPS_IFD)
        .and_then(|off| Ifd::read(b, 0, off).ok())
    {
        meta.gps = parse_gps(&gps);
    }
    Ok(meta)
}

fn dms(ifd: &Ifd, tag: u16) -> Option<f64> {
    let v = ifd.rationals(tag, 3)?;
    let [d, m, s] = v[..] else { return None };
    Some(d.to_f64()? + m.to_f64()? / 60.0 + s.to_f64().unwrap_or(0.0) / 3600.0)
}

fn parse_gps(ifd: &Ifd) -> Option<Gps> {
    let lat = dms(ifd, tag::GPS_LAT)?;
    let lon = dms(ifd, tag::GPS_LON)?;
    let sign = |r: Option<String>, neg: char| {
        if r.is_some_and(|s| s.starts_with(neg)) {
            -1.0
        } else {
            1.0
        }
    };
    let latitude = lat * sign(ifd.ascii(tag::GPS_LAT_REF), 'S');
    let longitude = lon * sign(ifd.ascii(tag::GPS_LON_REF), 'W');
    // (0, 0) is what some apps write when they have no fix.
    let plausible = latitude.abs() <= 90.0
        && longitude.abs() <= 180.0
        && !(latitude == 0.0 && longitude == 0.0);
    if !plausible || !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    let altitude_m = ifd
        .rational(tag::GPS_ALT)
        .and_then(Rational::to_f64)
        .map(|a| {
            let below = ifd.int(tag::GPS_ALT_REF) == Some(1);
            if below {
                -a
            } else {
                a
            }
        });
    let timestamp_utc = (|| {
        let date = ifd.ascii(tag::GPS_DATE)?;
        let t = ifd.rationals(tag::GPS_TIME, 3)?;
        let [h, m, s] = t[..] else { return None };
        let secs = s.to_f64()?;
        let mut dt = DateTime::parse_exif(&format!("{date} 00:00:00"))?;
        dt.hour = h.to_f64()? as u8;
        dt.minute = m.to_f64()? as u8;
        dt.second = secs as u8;
        (dt.hour < 24 && dt.minute < 60 && dt.second < 61).then_some(dt)
    })();
    Some(Gps {
        latitude,
        longitude,
        altitude_m,
        timestamp_utc,
    })
}

fn subsec_millis(s: &str) -> Option<u16> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).take(3).collect();
    if digits.is_empty() {
        return None;
    }
    let padded = format!("{digits:0<3}");
    padded.parse().ok()
}

/// Parses `+02:00` / `-05:00`.
fn parse_offset(s: &str) -> Option<i16> {
    let b = s.as_bytes();
    if b.len() < 6 || b[3] != b':' {
        return None;
    }
    let sign = match b[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let h: i16 = s.get(1..3)?.parse().ok()?;
    let m: i16 = s.get(4..6)?.parse().ok()?;
    (h <= 14 && m < 60).then_some(sign * (h * 60 + m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets() {
        assert_eq!(parse_offset("+02:00"), Some(120));
        assert_eq!(parse_offset("-04:30"), Some(-270));
        assert_eq!(parse_offset("   :  "), None);
    }

    #[test]
    fn exif_dates() {
        let d = DateTime::parse_exif("2026:09:15 14:02:11").unwrap();
        assert_eq!(d.to_iso(Some(-240)), "2026-09-15T14:02:11-04:00");
        assert_eq!(DateTime::parse_exif("0000:00:00 00:00:00"), None);
        assert_eq!(DateTime::parse_exif("    :  :     :  :  "), None);
    }

    #[test]
    fn subseconds() {
        assert_eq!(subsec_millis("5"), Some(500));
        assert_eq!(subsec_millis("123456"), Some(123));
        assert_eq!(subsec_millis(""), None);
    }

    #[test]
    fn film_codes() {
        assert_eq!(
            FilmSimulation::from_codes(Some(0x600), Some(0x8000)),
            Some(FilmSimulation::ClassicChrome)
        );
        assert_eq!(
            FilmSimulation::from_codes(Some(0x000), Some(0x501)),
            Some(FilmSimulation::Acros(MonoFilter::Red))
        );
        assert_eq!(
            FilmSimulation::from_codes(None, Some(0x310)),
            Some(FilmSimulation::Sepia)
        );
        assert_eq!(
            FilmSimulation::from_codes(Some(0xb00), Some(0)).map(|f| f.name()),
            Some("Reala Ace".into())
        );
        assert_eq!(FilmSimulation::from_codes(None, Some(0)), None);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert_eq!(read(b""), Err(Error::Truncated));
        assert_eq!(read(b"hello world, not a photo"), Err(Error::UnknownFormat));
        assert_eq!(read(&[0xFF, 0xD8, 0xFF]), Err(Error::Truncated));
        assert_eq!(read(&[0xFF, 0xD8, 0xFF, 0xDA, 0, 2]), Err(Error::NoExif));
    }
}
