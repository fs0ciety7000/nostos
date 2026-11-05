//! Minimal TIFF/IFD reader: enough to walk IFD0, the Exif and GPS sub-IFDs
//! and the Fujifilm maker note.

use crate::bytes::{Bytes, Endian};
use crate::{Error, Rational};

/// IFD entries larger than this are treated as corrupt. Real Fujifilm
/// maker notes have well under 200 entries.
const MAX_ENTRIES: u16 = 1024;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry {
    pub typ: u16,
    pub count: u32,
    /// Absolute position of the value bytes inside the buffer.
    pub at: usize,
}

pub(crate) struct Ifd<'a> {
    bytes: Bytes<'a>,
    entries: Vec<(u16, Entry)>,
}

fn type_size(typ: u16) -> Option<usize> {
    Some(match typ {
        1 | 2 | 6 | 7 => 1,   // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,           // SHORT, SSHORT
        4 | 9 | 11 | 13 => 4, // LONG, SLONG, FLOAT, IFD
        5 | 10 | 12 => 8,     // RATIONAL, SRATIONAL, DOUBLE
        _ => return None,
    })
}

/// Parses the TIFF header at the start of `data` and returns its byte order
/// and the offset of IFD0.
pub(crate) fn header(data: &[u8]) -> Result<(Endian, usize), Error> {
    let endian = match data.get(0..2) {
        Some(b"II") => Endian::Little,
        Some(b"MM") => Endian::Big,
        Some(_) => return Err(Error::Malformed("bad TIFF byte order")),
        None => return Err(Error::Truncated),
    };
    let b = Bytes::new(data, endian);
    if b.u16(2)? != 42 {
        return Err(Error::Malformed("bad TIFF magic"));
    }
    Ok((endian, b.u32(4)? as usize))
}

impl<'a> Ifd<'a> {
    /// Reads the IFD at `base + offset`. Value offsets inside the IFD are
    /// relative to `base` (the TIFF header, or the maker-note start for Fujifilm).
    pub fn read(bytes: Bytes<'a>, base: usize, offset: usize) -> Result<Self, Error> {
        let start = base.checked_add(offset).ok_or(Error::Truncated)?;
        let n = bytes.u16(start)?;
        if n > MAX_ENTRIES {
            return Err(Error::Malformed("too many IFD entries"));
        }
        let mut entries = Vec::with_capacity(n as usize);
        for i in 0..n as usize {
            let e = start + 2 + i * 12;
            let tag = bytes.u16(e)?;
            let typ = bytes.u16(e + 2)?;
            let count = bytes.u32(e + 4)?;
            let Some(size) = type_size(typ).and_then(|s| s.checked_mul(count as usize)) else {
                continue; // unknown type: skip the entry, keep the rest
            };
            let at = if size <= 4 {
                e + 8
            } else {
                match base.checked_add(bytes.u32(e + 8)? as usize) {
                    Some(at) => at,
                    None => continue,
                }
            };
            entries.push((tag, Entry { typ, count, at }));
        }
        Ok(Self { bytes, entries })
    }

    pub fn entry(&self, tag: u16) -> Option<Entry> {
        self.entries
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, e)| *e)
    }

    /// First value of an integer-typed tag, sign-extended.
    pub fn int(&self, tag: u16) -> Option<i64> {
        let e = self.entry(tag)?;
        let b = &self.bytes;
        let v = match e.typ {
            1 | 7 => i64::from(b.u8(e.at).ok()?),
            6 => i64::from(b.u8(e.at).ok()? as i8),
            3 => i64::from(b.u16(e.at).ok()?),
            8 => i64::from(b.u16(e.at).ok()? as i16),
            4 => i64::from(b.u32(e.at).ok()?),
            9 => i64::from(b.i32(e.at).ok()?),
            _ => return None,
        };
        (e.count >= 1).then_some(v)
    }

    /// Integer values of a tag (up to `max`).
    pub fn ints(&self, tag: u16, max: usize) -> Option<Vec<i64>> {
        let e = self.entry(tag)?;
        let size = type_size(e.typ)?;
        let n = (e.count as usize).min(max);
        let b = &self.bytes;
        (0..n)
            .map(|i| {
                let at = e.at + i * size;
                Some(match e.typ {
                    1 | 7 => i64::from(b.u8(at).ok()?),
                    6 => i64::from(b.u8(at).ok()? as i8),
                    3 => i64::from(b.u16(at).ok()?),
                    8 => i64::from(b.u16(at).ok()? as i16),
                    4 => i64::from(b.u32(at).ok()?),
                    9 => i64::from(b.i32(at).ok()?),
                    _ => return None,
                })
            })
            .collect()
    }

    /// First byte of the value read as a signed byte, whatever the declared
    /// type (exiftool's `Format => 'int8s'` override).
    pub fn first_i8(&self, tag: u16) -> Option<i8> {
        let e = self.entry(tag)?;
        self.bytes.u8(e.at).ok().map(|v| v as i8)
    }

    pub fn ascii(&self, tag: u16) -> Option<String> {
        let e = self.entry(tag)?;
        if e.typ != 2 && e.typ != 7 {
            return None;
        }
        let raw = self.bytes.slice(e.at, e.count as usize).ok()?;
        let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
        let s = String::from_utf8_lossy(&raw[..end]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    pub fn rationals(&self, tag: u16, max: usize) -> Option<Vec<Rational>> {
        let e = self.entry(tag)?;
        if e.typ != 5 && e.typ != 10 {
            return None;
        }
        let n = (e.count as usize).min(max);
        (0..n)
            .map(|i| {
                let at = e.at + i * 8;
                let (num, den) = if e.typ == 5 {
                    (
                        i64::from(self.bytes.u32(at).ok()?),
                        i64::from(self.bytes.u32(at + 4).ok()?),
                    )
                } else {
                    (
                        i64::from(self.bytes.i32(at).ok()?),
                        i64::from(self.bytes.i32(at + 4).ok()?),
                    )
                };
                Some(Rational { num, den })
            })
            .collect()
    }

    pub fn rational(&self, tag: u16) -> Option<Rational> {
        self.rationals(tag, 1)?.into_iter().next()
    }

    /// Offset stored in a LONG/IFD-typed pointer tag (Exif, GPS sub-IFDs).
    pub fn pointer(&self, tag: u16) -> Option<usize> {
        let e = self.entry(tag)?;
        matches!(e.typ, 4 | 13).then(|| self.bytes.u32(e.at).ok().map(|v| v as usize))?
    }
}
