//! Hostile-input and I/O-budget tests. No fixture byte pattern may make the
//! parser panic, and a RAF is read without touching its raw sensor data.

use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// xorshift: deterministic, no dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn truncation_at_any_point_never_panics() {
    let data = fixture("recipe-classic-chrome.jpg");
    for end in (0..data.len()).step_by(7) {
        let _ = nostos_exif::read(&data[..end]);
    }
}

#[test]
fn random_corruption_never_panics() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for name in ["x-t5.jpg", "recipe-acros-red.jpg", "gps-montmorency.jpg"] {
        let clean = fixture(name);
        for _ in 0..3000 {
            let mut d = clean.clone();
            let flips = 1 + (rng.next() % 12) as usize;
            for _ in 0..flips {
                // Bias corruption towards the Exif header and IFD tables.
                let span = if rng.next() % 2 == 0 {
                    2048.min(d.len())
                } else {
                    d.len()
                };
                let i = (rng.next() as usize) % span;
                d[i] = rng.next() as u8;
            }
            let _ = nostos_exif::read(&d);
            let _ = nostos_exif::read_from(&mut Cursor::new(&d));
        }
    }
}

/// Builds a RAF the way the camera lays it out: header, embedded JPEG, then
/// (here) 40 MB of fake sensor data.
fn fake_raf(jpeg: &[u8]) -> Vec<u8> {
    let mut raf = vec![0u8; 0x94];
    raf[..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
    raf[0x1C..0x1C + 4].copy_from_slice(b"X-T5");
    let off = raf.len() as u32;
    raf[84..88].copy_from_slice(&off.to_be_bytes());
    raf[88..92].copy_from_slice(&(jpeg.len() as u32).to_be_bytes());
    raf.extend_from_slice(jpeg);
    raf.resize(raf.len() + 40 * 1024 * 1024, 0xAB);
    raf
}

struct Counting<R> {
    inner: R,
    read: u64,
}
impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}
impl<R: Seek> Seek for Counting<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[test]
fn raf_is_read_through_its_preview_only() {
    let jpeg = fixture("recipe-classic-chrome.jpg");
    let raf = fake_raf(&jpeg);
    let mut src = Counting {
        inner: Cursor::new(&raf),
        read: 0,
    };
    let m = nostos_exif::read_from(&mut src).expect("raf");
    assert_eq!(m.container, nostos_exif::Container::Raf);
    assert_eq!(
        m.fuji.and_then(|f| f.film_simulation).map(|f| f.name()),
        Some("Classic Chrome".into())
    );
    assert!(
        src.read < 300 * 1024,
        "read {} bytes of a {} byte RAF",
        src.read,
        raf.len()
    );
    // In-memory path gives the same answer.
    assert_eq!(
        nostos_exif::read(&raf).unwrap().fuji,
        nostos_exif::read(&jpeg).unwrap().fuji
    );
}

#[test]
fn jpeg_without_exif_is_reported_as_such() {
    // The fixture's own 8x8 image, with the Exif segment stripped.
    let with = fixture("x-t5.jpg");
    let app1_len = u16::from_be_bytes([with[4], with[5]]) as usize;
    let mut without = with[..2].to_vec();
    without.extend_from_slice(&with[4 + app1_len..]);
    assert_eq!(nostos_exif::read(&without), Err(nostos_exif::Error::NoExif));
}
