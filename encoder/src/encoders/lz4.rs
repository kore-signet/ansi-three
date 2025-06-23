use std::io::{self};

use arrayvec::ArrayVec;
use container::{metadata::CompressionMode, side_data};
use lz4_flex::{
    block::{compress_into_with_dict, get_maximum_output_size},
    compress_into,
};

use crate::encoders::PostProcessor;

#[derive(Default)]
pub struct Lz4Compressor {
    dict: Option<Vec<u8>>,
    scratch: Vec<u8>,
}

impl Lz4Compressor {
    pub fn with_dict(dict: impl AsRef<[u8]>) -> Self {
        Lz4Compressor {
            dict: Some(dict.as_ref().to_vec()),
            scratch: Vec::new(),
        }
    }
}

impl PostProcessor for Lz4Compressor {
    fn post_process(
        &mut self,
        packet: &mut container::Packet,
        data: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        self.scratch.clear();
        self.scratch.resize(get_maximum_output_size(data.len()), 0);

        let uncompressed_len = data.len();
        let compressed_data_len = match self.dict.as_ref() {
            Some(dict) => compress_into_with_dict(data, &mut self.scratch, dict),
            None => compress_into(data, &mut self.scratch),
        }
        .map_err(io::Error::other)?;

        self.scratch.truncate(compressed_data_len);

        packet.side_data.insert(
            side_data::DECOMPRESSED_LEN,
            ArrayVec::from_iter((uncompressed_len as u64).to_le_bytes()),
        );
        packet.side_data.insert(
            side_data::COMPRESSION_METHOD,
            ArrayVec::from_iter([CompressionMode::Lz4 as u8]),
        );

        packet.data_len = compressed_data_len as u64;

        data.clear();
        data.append(&mut self.scratch);

        Ok(())
    }
}
