use std::io::{self};

use arrayvec::ArrayVec;
use container::{metadata::CompressionMode, side_data};
use zstd::{bulk::Compressor, zstd_safe};

use crate::encoders::PostProcessor;

pub struct ZstdCompressor {
    compressor: zstd::bulk::Compressor<'static>,
    scratch: Vec<u8>,
}

impl ZstdCompressor {
    pub fn new(level: i32) -> io::Result<Self> {
        Ok(ZstdCompressor {
            compressor: Compressor::new(level)?,
            scratch: Vec::new(),
        })
    }

    pub fn with_dict(level: i32, dict: impl AsRef<[u8]>) -> io::Result<Self> {
        Ok(ZstdCompressor {
            compressor: Compressor::with_dictionary(level, dict.as_ref())?,
            scratch: Vec::new(),
        })
    }
}

impl PostProcessor for ZstdCompressor {
    fn post_process(
        &mut self,
        packet: &mut container::Packet,
        data: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        self.scratch.clear();
        self.scratch.reserve(zstd_safe::compress_bound(data.len()));

        // self.scratch.resize(get_maximum_output_size(data.len()), 0);

        let uncompressed_len = data.len();
        let compressed_data_len = self
            .compressor
            .compress_to_buffer(data, &mut self.scratch)?;
        self.scratch.truncate(compressed_data_len);

        packet.side_data.insert(
            side_data::DECOMPRESSED_LEN,
            ArrayVec::from_iter((uncompressed_len as u64).to_le_bytes()),
        );
        packet.side_data.insert(
            side_data::COMPRESSION_METHOD,
            ArrayVec::from_iter([CompressionMode::Zstd as u8]),
        );

        packet.data_len = compressed_data_len as u64;

        data.clear();
        data.append(&mut self.scratch);

        Ok(())
    }
}
