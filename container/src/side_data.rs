use std::{
    io::{Read, Write},
    ops::{Deref, DerefMut},
};

use arrayvec::ArrayVec;
use byteorder::{ReadBytesExt, WriteBytesExt};
use litemap::LiteMap;

use crate::EncodableData;

use std::fmt::Display;

#[derive(PartialEq, PartialOrd, Eq, Ord, Clone, Copy, Debug)]
pub struct Tag {
    inner: [u8; 4],
}

impl Tag {
    const unsafe fn new_unchecked(tag: [u8; 4]) -> Tag {
        Tag { inner: tag }
    }

    fn new_checked(tag: [u8; 4]) -> Tag {
        for val in tag {
            assert!(val.is_ascii_graphic());
        }

        Tag { inner: tag }
    }
}

impl Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{}>", std::str::from_utf8(&self.inner).unwrap())
    }
}

pub const COMPRESSION_METHOD: Tag = unsafe { Tag::new_unchecked([b'C', b'M', b'P', b'M']) };
pub const DECOMPRESSED_LEN: Tag = unsafe { Tag::new_unchecked([b'D', b'C', b'L', b'E']) };

#[repr(transparent)]
#[derive(Default, Debug, PartialEq, Clone)]
pub struct SideData {
    inner: LiteMap<Tag, ArrayVec<u8, 256>>,
}

impl EncodableData for SideData {
    fn estimated_size(&self) -> Option<usize> {
        Some(self.inner.iter().fold(
            1, /* length marker */
            |acc, e| acc + 4 /* tag */ + 1 /* length marker */ + e.1.len(),
        ))
    }

    fn encode_into<W: Write>(&self, out: &mut W) -> std::io::Result<u64> {
        let mut total_bytes: u64 = 0;

        out.write_u8(self.inner.len() as u8)?;
        total_bytes += 1;

        for (key, val) in self.inner.iter() {
            out.write_all(&key.inner)?;
            out.write_u8(val.len() as u8)?;
            total_bytes += 5;

            out.write_all(val)?;
            total_bytes += val.len() as u64;
        }

        Ok(total_bytes)
    }

    fn decode_from<R: Read>(input: &mut R) -> std::io::Result<Self> {
        let len = input.read_u8()?;

        let mut data = LiteMap::with_capacity(len as usize);

        for _ in 0..len {
            let mut tag: [u8; 4] = [0; 4];
            input.read_exact(&mut tag)?;

            let marker = input.read_u8()?;

            let mut buf = ArrayVec::new();

            // SAFETY: capacity is guaranteed to be 256, equal to max data that can be read since marker: u8
            let slice =
                unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr(), marker as usize) };
            input.read_exact(slice)?;

            // SET initialized elements to = <marker> elements read
            unsafe { buf.set_len(marker as usize) };

            data.insert(unsafe { Tag::new_unchecked(tag) }, buf);
        }

        Ok(SideData::from(data))
    }
}

impl Display for SideData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Side Data {{")?;
        for (key, val) in &self.inner {
            writeln!(f, "\t\t{key} => {val:?}")?;
        }
        writeln!(f, "\t}}")
    }
}

impl From<LiteMap<Tag, ArrayVec<u8, 256>>> for SideData {
    fn from(value: LiteMap<Tag, ArrayVec<u8, 256>>) -> Self {
        Self { inner: value }
    }
}

impl Deref for SideData {
    type Target = LiteMap<Tag, ArrayVec<u8, 256>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for SideData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
