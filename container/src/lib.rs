extern crate alloc;

use std::{
    fmt::Display,
    io::{self, Read, Write},
    time::Duration,
};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use typed_builder::TypedBuilder;

use crate::side_data::SideData;

pub mod metadata;
pub mod seek;
pub mod side_data;

pub struct FormatDuration(pub Duration);

impl Display for FormatDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seconds = self.0.as_secs() % 60;
        let minutes = (self.0.as_secs() / 60) % 60;
        let hours = (self.0.as_secs() / 60) / 60;
        let frac_secs = self.0.subsec_millis();
        write!(f, "{hours:0>2}:{minutes:0>2}:{seconds:0>2}.{frac_secs:0>3}")
    }
}

/*

File Format!

-- (marker: len_bytes, u64) Header: DER-encoded FormatData
-- (marker: amount of seektables, u8) Seek Tables
    -- (stream_index: u8)
    -- (seek_table_length: u64 / bytes)
-- (interleaved packet data)

*/

pub trait EncodableData: Sized {
    fn estimated_size(&self) -> Option<usize>;

    /// returns bytes written
    fn encode_into<W: Write>(&self, out: &mut W) -> std::io::Result<u64>;

    fn decode_from<R: Read>(input: &mut R) -> std::io::Result<Self>;

    fn encode_to_vec(&self) -> Vec<u8> {
        let mut vec = Vec::with_capacity(self.estimated_size().unwrap_or(256));
        self.encode_into(&mut vec).unwrap();
        vec
    }
}

pub trait TypedData: EncodableData {
    const KIND: PacketDataType;
}

#[repr(u8)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum PacketDataType {
    Video = 0,
    Audio = 1,
    Subtitle = 2,
    Unknown = 3,
    Invalid = 255,
}

impl TryFrom<u8> for PacketDataType {
    type Error = std::io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => PacketDataType::Video,
            1 => PacketDataType::Audio,
            2 => PacketDataType::Subtitle,
            3 => PacketDataType::Unknown,
            255 => PacketDataType::Invalid,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "")),
        })
    }
}

impl Default for PacketDataType {
    fn default() -> Self {
        Self::Invalid
    }
}

#[derive(Debug, PartialEq, Clone, Default, TypedBuilder)]
pub struct Packet {
    pub stream: u8,
    #[builder(default, setter(skip))]
    pub packet_idx: u64,
    pub timestamp: Duration, // micros as u64
    pub duration: Duration,  // micros as u64
    #[builder(default, setter(into))]
    pub side_data: SideData,
    #[builder(default, setter(skip))]
    pub data_type: PacketDataType,
    #[builder(default, setter(skip))]
    pub data_len: u64,
}

impl Display for Packet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Packet {{")?;
        writeln!(f, "\tstream => {}", self.stream)?;
        writeln!(f, "\tidx => {}", self.packet_idx)?;
        writeln!(f, "\ttimestamp => {}", FormatDuration(self.timestamp))?;
        writeln!(f, "\tduration => {}", FormatDuration(self.duration))?;
        writeln!(f, "\tdata_type => {:?}", self.data_type)?;
        writeln!(f, "\tdata_len => {}", self.data_len)?;
        writeln!(f, "\tside_data => {}", self.side_data)?;
        writeln!(f, "}}")
    }
}

impl EncodableData for Packet {
    fn estimated_size(&self) -> Option<usize> {
        Some(
            1 // stream idx
            + 8 // packet-idx
            + 8 + 8 // timestamp + duration
            + self.side_data.estimated_size().unwrap()
            + 1 // data type
            + 8, // len
        )
    }

    fn encode_into<W: Write>(&self, out: &mut W) -> std::io::Result<u64> {
        let mut total_bytes = 0u64;

        out.write_u8(self.stream)?;
        out.write_u64::<LittleEndian>(self.packet_idx)?;
        out.write_u64::<LittleEndian>(self.timestamp.as_micros() as u64)?;
        out.write_u64::<LittleEndian>(self.duration.as_micros() as u64)?;
        total_bytes += 8 * 3 + 1;

        total_bytes += self.side_data.encode_into(out)?;

        out.write_u8(self.data_type as u8)?;
        out.write_u64::<LittleEndian>(self.data_len)?;
        total_bytes += 9;

        Ok(total_bytes)
    }

    fn decode_from<R: Read>(input: &mut R) -> std::io::Result<Self> {
        let stream = input.read_u8()?;
        let packet_idx = input.read_u64::<LittleEndian>()?;
        let timestamp = input.read_u64::<LittleEndian>()?;
        let duration = input.read_u64::<LittleEndian>()?;
        let side_data = SideData::decode_from(input)?;
        let data_type = input.read_u8()?;
        let data_len = input.read_u64::<LittleEndian>()?;

        Ok(Packet {
            stream,
            packet_idx,
            timestamp: Duration::from_micros(timestamp),
            duration: Duration::from_micros(duration),
            side_data,
            data_type: PacketDataType::try_from(data_type)?,
            data_len,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct SubRect {
    pub x: i16,
    pub y: i16,
    pub fg: u8, // in ansi codes
    pub bg: u8, // in ansi codes
    pub text: String,
}

impl EncodableData for SubRect {
    fn estimated_size(&self) -> Option<usize> {
        Some(
            2 + 2 // x, y
            + 1 + 1 // fg + bg
            + 4 // text length marker
            + self.text.len(), // text length
        )
    }

    fn encode_into<W: Write>(&self, out: &mut W) -> std::io::Result<u64> {
        out.write_i16::<LittleEndian>(self.x)?;
        out.write_i16::<LittleEndian>(self.y)?;
        out.write_u8(self.fg)?;
        out.write_u8(self.bg)?;
        out.write_u32::<LittleEndian>(self.text.len() as u32)?;

        out.write_all(self.text.as_bytes())?;
        Ok(
            2 + 2 // x, y
            + 1 + 1 // fg + bg
            + 4 // text length marker
            + self.text.len() as u64, // text length)
        )
    }

    fn decode_from<R: Read>(input: &mut R) -> std::io::Result<Self> {
        let x = input.read_i16::<LittleEndian>()?;
        let y = input.read_i16::<LittleEndian>()?;
        let fg = input.read_u8()?;
        let bg = input.read_u8()?;
        let text_len = input.read_u32::<LittleEndian>()?;
        let mut buf = vec![0u8; text_len as usize];
        input.read_exact(&mut buf)?;

        Ok(SubRect {
            x,
            y,
            fg,
            bg,
            text: String::from_utf8(buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        })
    }
}

#[derive(Debug, Clone, Default)]
#[repr(transparent)]
pub struct SubRectVec {
    pub inner: Vec<SubRect>,
}

impl SubRectVec {
    pub fn into_inner(self) -> Vec<SubRect> {
        self.inner
    }
}

impl From<Vec<SubRect>> for SubRectVec {
    fn from(value: Vec<SubRect>) -> Self {
        Self { inner: value }
    }
}

impl EncodableData for SubRectVec {
    fn estimated_size(&self) -> Option<usize> {
        Some(self.inner.iter().fold(2, |total, element| {
            total + element.estimated_size().unwrap()
        }))
    }

    fn encode_into<W: Write>(&self, out: &mut W) -> std::io::Result<u64> {
        let mut total_bytes = 2u64;
        out.write_u16::<LittleEndian>(self.inner.len() as u16)?;
        for rect in &self.inner {
            total_bytes += rect.encode_into(out)?;
        }

        Ok(total_bytes)
    }

    fn decode_from<R: Read>(input: &mut R) -> std::io::Result<Self> {
        let len = input.read_u16::<LittleEndian>()?;
        let mut rects = Vec::with_capacity(len as usize);
        for _ in 0..len {
            rects.push(SubRect::decode_from(input)?);
        }

        Ok(SubRectVec { inner: rects })
    }
}

impl TypedData for SubRectVec {
    const KIND: PacketDataType = PacketDataType::Subtitle;
}

impl SubRect {
    pub fn to_string(&self) -> String {
        format!(
            "\x1b[{};{}H\x1b[38;5;{}m\x1b[48;5;{}m{}",
            self.y, self.x, self.fg, self.bg, self.text
        )
    }
    //
}
