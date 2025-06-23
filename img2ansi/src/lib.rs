#![allow(unused_must_use)]

use std::{
    io::{self, Read, Write},
    ops::Deref,
};

use byteorder::WriteBytesExt;
use colorful::palette::*;
use container::{EncodableData, PacketDataType, TypedData};
use image::{GenericImageView, Luma, Rgb};

pub trait AnsiPixel: PartialEq {
    fn fg_code(&self, out: &mut impl Write) -> std::io::Result<()>;
    fn bg_code(&self, out: &mut impl Write) -> std::io::Result<()>;
}

impl AnsiPixel for Luma<u8> {
    fn fg_code(&self, out: &mut impl Write) -> std::io::Result<()> {
        out.write_all(PALETTE_FG_CODES[self.0[0] as usize].as_bytes())
    }

    fn bg_code(&self, out: &mut impl Write) -> std::io::Result<()> {
        out.write_all(PALETTE_BG_CODES[self.0[0] as usize].as_bytes())
    }
}

impl AnsiPixel for Rgb<u8> {
    fn fg_code(&self, out: &mut impl Write) -> std::io::Result<()> {
        out.write_all(b"\x1b[38;2;")?;

        let mut buffer = itoa::Buffer::new();
        out.write_all(buffer.format(self.0[0]).as_bytes())?;
        out.write_u8(b';')?;
        out.write_all(buffer.format(self.0[1]).as_bytes())?;
        out.write_u8(b';')?;
        out.write_all(buffer.format(self.0[2]).as_bytes())?;
        out.write_u8(b'm')?;

        Ok(())
    }

    fn bg_code(&self, out: &mut impl Write) -> std::io::Result<()> {
        out.write_all(b"\x1b[48;2;")?;

        let mut buffer = itoa::Buffer::new();

        out.write_all(buffer.format(self.0[0]).as_bytes())?;
        out.write_u8(b';')?;
        out.write_all(buffer.format(self.0[1]).as_bytes())?;
        out.write_u8(b';')?;
        out.write_all(buffer.format(self.0[2]).as_bytes())?;
        out.write_u8(b'm')?;

        Ok(())
    }
}

#[repr(transparent)]
pub struct AnsiFrame<T: ToAnsi> {
    inner: T,
}

impl<T: ToAnsi> AnsiFrame<T> {
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: ToAnsi> From<T> for AnsiFrame<T> {
    fn from(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: ToAnsi> Deref for AnsiFrame<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ToAnsi> EncodableData for AnsiFrame<T> {
    fn estimated_size(&self) -> Option<usize> {
        self.est_size()
    }

    fn encode_into<W: Write>(&self, out: &mut W) -> std::io::Result<u64> {
        self.to_ansi(out)?;
        Ok(0)
    }

    fn decode_from<R: Read>(_: &mut R) -> std::io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported, ""))
    }
}

impl<T: ToAnsi> TypedData for AnsiFrame<T> {
    const KIND: PacketDataType = PacketDataType::Video;
}

pub trait ToAnsi {
    fn to_ansi(&self, frame: &mut impl Write) -> std::io::Result<()>;

    fn est_size(&self) -> Option<usize> {
        None
    }
}

impl<T> ToAnsi for T
where
    T: GenericImageView<Pixel: AnsiPixel>,
{
    fn to_ansi(&self, frame: &mut impl Write) -> std::io::Result<()> {
        let mut last_upper: Option<T::Pixel> = None;
        let mut last_lower: Option<T::Pixel> = None;

        for y in (0..self.height() - 1).step_by(2) {
            for x in 0..self.width() {
                let upper = self.get_pixel(x, y);
                let lower = self.get_pixel(x, y + 1);

                if last_upper.is_none_or(|v| v != upper) {
                    upper.fg_code(frame);
                }

                if last_lower.is_none_or(|v| v != lower) {
                    lower.bg_code(frame);
                }

                frame.write_all(b"\xE2\x96\x80");

                last_upper = Some(upper);
                last_lower = Some(lower);
            }

            frame.write_all(b"\x1b[1E");
        }

        Ok(())
    }

    fn est_size(&self) -> Option<usize> {
        Some(self.width() as usize * self.height() as usize * 20)
    }
}
