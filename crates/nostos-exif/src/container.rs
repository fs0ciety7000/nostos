//! Locates the TIFF/Exif block inside a JPEG, a Fujifilm RAF or a bare TIFF.

use crate::bytes::{be_u16, be_u32};
use crate::Error;

/// RAF files start with this magic, followed by a header whose bytes 84..92
/// hold the big-endian offset and length of the embedded full-size JPEG preview.
pub(crate) const RAF_MAGIC: &[u8] = b"FUJIFILMCCD-RAW";
pub(crate) const RAF_JPEG_PTR: usize = 84;

/// File container detected from the first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Container {
    Jpeg,
    Raf,
    Tiff,
}

pub(crate) fn detect(data: &[u8]) -> Result<Container, Error> {
    if data.starts_with(&[0xFF, 0xD8]) {
        Ok(Container::Jpeg)
    } else if data.starts_with(RAF_MAGIC) {
        Ok(Container::Raf)
    } else if data.starts_with(b"II*\0") || data.starts_with(b"MM\0*") {
        Ok(Container::Tiff)
    } else if data.len() < 16 {
        Err(Error::Truncated)
    } else {
        Err(Error::UnknownFormat)
    }
}

/// Returns `(offset, len)` of the JPEG preview embedded in a RAF header.
pub(crate) fn raf_jpeg(header: &[u8]) -> Result<(usize, usize), Error> {
    let off = be_u32(header, RAF_JPEG_PTR)? as usize;
    let len = be_u32(header, RAF_JPEG_PTR + 4)? as usize;
    if off == 0 || len == 0 {
        return Err(Error::Malformed("RAF without JPEG preview"));
    }
    Ok((off, len))
}

/// Finds the TIFF block of the APP1 "Exif" segment in a JPEG stream.
pub(crate) fn jpeg_exif(data: &[u8]) -> Result<&[u8], Error> {
    let mut p = 2;
    loop {
        // Skip fill bytes between segments.
        while data.get(p) == Some(&0xFF) && data.get(p + 1) == Some(&0xFF) {
            p += 1;
        }
        let marker = be_u16(data, p)?;
        if marker & 0xFF00 != 0xFF00 {
            return Err(Error::Malformed("bad JPEG marker"));
        }
        match marker {
            // Standalone markers carry no length.
            0xFF01 | 0xFFD0..=0xFFD7 => {
                p += 2;
                continue;
            }
            // Start of scan or end of image: no Exif before the image data.
            0xFFDA | 0xFFD9 => return Err(Error::NoExif),
            _ => {}
        }
        let len = be_u16(data, p + 2)? as usize;
        if len < 2 {
            return Err(Error::Malformed("bad JPEG segment length"));
        }
        let body = p + 4;
        if marker == 0xFFE1 && data.get(body..body + 6) == Some(b"Exif\0\0") {
            let end = p + 2 + len;
            return data.get(body + 6..end).ok_or(Error::Truncated);
        }
        p += 2 + len;
    }
}

/// Finds the TIFF block in an in-memory file of any supported container.
pub(crate) fn find_tiff(data: &[u8]) -> Result<(Container, &[u8]), Error> {
    let kind = detect(data)?;
    let tiff = match kind {
        Container::Jpeg => jpeg_exif(data)?,
        Container::Tiff => data,
        Container::Raf => {
            let (off, len) = raf_jpeg(data)?;
            let end = off.saturating_add(len).min(data.len());
            let jpeg = data.get(off..end).ok_or(Error::Truncated)?;
            if !jpeg.starts_with(&[0xFF, 0xD8]) {
                return Err(Error::Malformed("RAF preview is not a JPEG"));
            }
            jpeg_exif(jpeg)?
        }
    };
    Ok((kind, tiff))
}
