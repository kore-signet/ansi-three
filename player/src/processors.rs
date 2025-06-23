use std::io;

use container::{Packet, side_data};
use lz4_flex::{block::decompress_into_with_dict, decompress_into};
use zstd::bulk::Decompressor;

pub trait DecoderProcessor {
    fn process(&mut self, packet: &mut Packet, data: &mut Vec<u8>) -> io::Result<()>;
}

#[derive(Default)]
pub struct Lz4Decoder {
    dict: Option<Vec<u8>>,
    scratch: Vec<u8>,
}

impl Lz4Decoder {
    pub fn new(dict: Option<impl AsRef<[u8]>>) -> Self {
        Lz4Decoder {
            dict: dict.map(|v| v.as_ref().to_vec()),
            scratch: Vec::new(),
        }
    }
}

impl DecoderProcessor for Lz4Decoder {
    fn process(&mut self, packet: &mut Packet, data: &mut Vec<u8>) -> io::Result<()> {
        let decompressed_len = packet
            .side_data
            .get(&side_data::DECOMPRESSED_LEN)
            .and_then(|v| v.as_slice().try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(io::Error::new(
                io::ErrorKind::InvalidInput,
                "side data: decompressed len is missing",
            ))?;

        self.scratch.clear();
        self.scratch.resize(decompressed_len as usize, 0);

        match self.dict.as_ref() {
            Some(dict) => decompress_into_with_dict(data, &mut self.scratch, dict),
            None => decompress_into(data, &mut self.scratch),
        }
        .map_err(|e| io::Error::other(e))?;

        data.clear();
        data.append(&mut self.scratch);

        Ok(())
    }
}

pub struct ZstdDecoder {
    decompressor: zstd::bulk::Decompressor<'static>,
    scratch: Vec<u8>,
}

impl ZstdDecoder {
    pub fn new(dict: Option<impl AsRef<[u8]>>) -> io::Result<Self> {
        Ok(ZstdDecoder {
            decompressor: match dict {
                Some(v) => Decompressor::with_dictionary(v.as_ref())?,
                None => Decompressor::new()?,
            },
            scratch: Vec::new(),
        })
    }
}

impl DecoderProcessor for ZstdDecoder {
    fn process(&mut self, packet: &mut Packet, data: &mut Vec<u8>) -> io::Result<()> {
        let decompressed_len = packet
            .side_data
            .get(&side_data::DECOMPRESSED_LEN)
            .and_then(|v| v.as_slice().try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(io::Error::new(
                io::ErrorKind::InvalidInput,
                "side data: decompressed len is missing",
            ))?;

        self.scratch.clear();
        self.scratch.reserve(decompressed_len as usize);

        self.decompressor
            .decompress_to_buffer(data, &mut self.scratch)?;

        data.clear();
        data.append(&mut self.scratch);

        Ok(())
    }
}
