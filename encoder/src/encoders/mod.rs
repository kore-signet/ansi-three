use crate::ff::packet::FFPacket;
use arrayvec::ArrayVec;
use byteorder::{LittleEndian, WriteBytesExt};
use container::{
    Packet as AnsiPacket,
    seek::{SeekEntry, delta_encode},
};
use tsz_compress::prelude::TszCompressV2;

pub mod lz4;
pub mod subtitles;
pub mod video;
pub mod zstd;

pub trait FFToAnsi {
    fn process(
        &mut self,
        input: &FFPacket,
        packet: &mut AnsiPacket,
        data: &mut Vec<u8>,
    ) -> std::io::Result<()>;
}

pub trait PostProcessor {
    fn post_process(&mut self, packet: &mut AnsiPacket, data: &mut Vec<u8>) -> std::io::Result<()>;
}

pub struct Pipeline {
    start: Box<dyn FFToAnsi + Send>,
    post_steps: ArrayVec<Box<dyn PostProcessor + Send>, 8>,
}

impl Pipeline {
    pub fn new(start: impl FFToAnsi + Send + 'static) -> Pipeline {
        Pipeline {
            start: Box::new(start),
            post_steps: ArrayVec::new(),
        }
    }

    pub fn with_step(mut self, step: impl PostProcessor + Send + 'static) -> Pipeline {
        self.post_steps.push(Box::new(step));
        self
    }

    pub fn run(
        &mut self,
        input: &FFPacket,
        packet: &mut AnsiPacket,
        data: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        self.start.process(input, packet, data)?;
        for step in self.post_steps.iter_mut() {
            step.post_process(packet, data)?;
        }

        Ok(())
    }
}

pub struct SeekTableEncoder {
    stream_index: u8,
    resolution: u64,    // every n millis,
    last_recorded: u64, // n millis
    entries: Vec<SeekEntry>,
}

impl SeekTableEncoder {
    pub fn new(stream_index: u8) -> Self {
        Self {
            stream_index,
            resolution: 100,
            last_recorded: Default::default(),
            entries: Vec::with_capacity(32_000),
        }
    }

    pub fn set_stream_index(&mut self, stream_index: u8) {
        self.stream_index = stream_index;
    }
}

impl SeekTableEncoder {
    pub fn ingest(&mut self, packet: &AnsiPacket, position: u64) {
        if packet.timestamp.as_millis() == 0
            || packet.timestamp.as_millis() as u64 - self.last_recorded >= self.resolution
        {
            self.entries.push(SeekEntry {
                ts: packet.timestamp.as_micros() as i64,
                location: position as i64,
            });
            self.last_recorded = packet.timestamp.as_millis() as u64;
        }
    }

    pub fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();

        let (timestamps, locations): (Vec<i64>, Vec<i64>) = self
            .entries
            .into_iter()
            .map(|SeekEntry { ts, location }| (ts, location))
            .unzip();

        let len_elements = timestamps.len();

        let mut encoded = delta_encode(timestamps.into_iter());
        let mut encoded_locations = delta_encode(locations.into_iter());

        encoded.append(&mut encoded_locations);

        let mut compressed = lz4_flex::compress_prepend_size(&encoded);
        let len_bytes = compressed.len();

        out.write_u8(self.stream_index).unwrap();
        out.write_u64::<LittleEndian>(len_bytes as u64).unwrap();
        out.write_u64::<LittleEndian>(len_elements as u64).unwrap();
        out.append(&mut compressed);

        out
    }
}

// struct DeltaEncoder<I: Iterator<Item = u64>> {
//     prev_ts: u64,
//     prev_delta: u64,
//     inner: I
// }

// impl De
