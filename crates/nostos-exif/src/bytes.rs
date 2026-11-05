//! Bounds-checked big/little-endian reads over a byte slice.
//!
//! Every read returns `Err(Error::Truncated)` instead of panicking, so a
//! corrupt or hostile file can never crash the parser.

use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Endian {
    Little,
    Big,
}

#[derive(Clone, Copy)]
pub(crate) struct Bytes<'a> {
    pub data: &'a [u8],
    pub endian: Endian,
}

impl<'a> Bytes<'a> {
    pub fn new(data: &'a [u8], endian: Endian) -> Self {
        Self { data, endian }
    }

    pub fn slice(&self, offset: usize, len: usize) -> Result<&'a [u8], Error> {
        let end = offset.checked_add(len).ok_or(Error::Truncated)?;
        self.data.get(offset..end).ok_or(Error::Truncated)
    }

    fn array<const N: usize>(&self, offset: usize) -> Result<[u8; N], Error> {
        let s = self.slice(offset, N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(s);
        Ok(out)
    }

    pub fn u8(&self, offset: usize) -> Result<u8, Error> {
        self.data.get(offset).copied().ok_or(Error::Truncated)
    }

    pub fn u16(&self, offset: usize) -> Result<u16, Error> {
        let b = self.array::<2>(offset)?;
        Ok(match self.endian {
            Endian::Little => u16::from_le_bytes(b),
            Endian::Big => u16::from_be_bytes(b),
        })
    }

    pub fn u32(&self, offset: usize) -> Result<u32, Error> {
        let b = self.array::<4>(offset)?;
        Ok(match self.endian {
            Endian::Little => u32::from_le_bytes(b),
            Endian::Big => u32::from_be_bytes(b),
        })
    }

    pub fn i32(&self, offset: usize) -> Result<i32, Error> {
        self.u32(offset).map(|v| v as i32)
    }
}

/// Big-endian u32 read used by container headers (RAF), independent of TIFF order.
pub(crate) fn be_u32(data: &[u8], offset: usize) -> Result<u32, Error> {
    Bytes::new(data, Endian::Big).u32(offset)
}

pub(crate) fn be_u16(data: &[u8], offset: usize) -> Result<u16, Error> {
    Bytes::new(data, Endian::Big).u16(offset)
}
