#![feature(write_all_vectored)]

use std::{
    fmt::Display,
    io::{Cursor, Read, Seek},
    marker::PhantomData,
    time::Duration,
};

use byteorder::{LittleEndian, ReadBytesExt};
use container::{
    EncodableData, Packet,
    metadata::{CompressionMode, FormatData},
    seek::{SeekEntry, delta_decode},
};
use litemap::LiteMap;
use thingbuf::{Recycle, mpsc, recycling::WithCapacity};
use tsz_compress::prelude::TszDecompressV2;

use crate::processors::{DecoderProcessor, Lz4Decoder, ZstdDecoder};

pub mod processors;
pub mod renderer;

pub struct PacketWithData {
    pub header: Packet,
    pub data: Vec<u8>,
}

impl Recycle<PacketWithData> for WithCapacity {
    fn new_element(&self) -> PacketWithData {
        PacketWithData {
            header: Packet::default(),
            data: Vec::with_capacity(self.min_capacity()),
        }
    }

    fn recycle(&self, element: &mut PacketWithData) {
        element.data.shrink_to(self.max_capacity());
        element.data.clear();
    }
}

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

pub mod states {
    pub struct Start;
    pub struct HeaderRead;
    pub struct SeektablesRead;
}

pub struct Reader<R: Read, S> {
    reader: R,
    scratch: Vec<u8>,
    decoders: LiteMap<u8, Box<dyn DecoderProcessor + Send>>,
    seektable: Vec<SeekEntry>,
    start_of_packets: u64,
    last_time: i64,
    _spooky: PhantomData<S>,
}

impl<R: Read + Seek> Reader<R, states::Start> {
    pub fn new(reader: R) -> Reader<R, states::Start> {
        Reader {
            reader,
            scratch: Vec::with_capacity(192 * 108 * 20),
            decoders: LiteMap::new(),
            seektable: Vec::new(),
            start_of_packets: 0,
            last_time: 0,
            _spooky: PhantomData,
        }
    }

    pub fn read_header(mut self) -> anyhow::Result<(Reader<R, states::HeaderRead>, FormatData)> {
        self.scratch.clear();
        let header_len = self.reader.read_u64::<LittleEndian>()?;
        self.scratch.resize(header_len as usize, 0);
        self.reader.read_exact(&mut self.scratch)?;
        let header = rasn::der::decode::<FormatData>(&self.scratch)?;

        for stream in &header.tracks {
            match stream.compression_mode {
                CompressionMode::None => continue,
                CompressionMode::Zstd => self.decoders.insert(
                    stream.index as u8,
                    Box::new(ZstdDecoder::new(stream.compression_dict.as_ref())?),
                ),
                CompressionMode::Lz4 => self.decoders.insert(
                    stream.index as u8,
                    Box::new(Lz4Decoder::new(stream.compression_dict.as_ref())),
                ),
            };
        }

        Ok((
            Reader {
                start_of_packets: self.reader.stream_position()?,
                reader: self.reader,
                scratch: self.scratch,
                decoders: self.decoders,
                seektable: Vec::new(),
                last_time: 0,
                _spooky: PhantomData,
            },
            header,
        ))
    }
}

impl<R: Read + Seek> Reader<R, states::HeaderRead> {
    pub fn read_seektables(
        mut self,
    ) -> anyhow::Result<(Reader<R, states::SeektablesRead>, Vec<(u8, Vec<SeekEntry>)>)> {
        let n_seektables = self.reader.read_u8()?;
        let mut seektables: Vec<(u8, Vec<SeekEntry>)> = Vec::with_capacity(n_seektables as usize);

        for _ in 0..n_seektables {
            //   let (timestamps, locations): (Vec<i64>, Vec<i64>) = self.entries.into_iter().map(|SeekEntry { ts, location }| (ts, location)).unzip();

            //         let len_elements = timestamps.len();

            //         let mut encoded = delta_encode(timestamps.into_iter());
            //         let mut encoded_locations = delta_encode(locations.into_iter());

            //         encoded.append(&mut encoded_locations);

            //         let compressed = lz4_flex::compress_prepend_size(&encoded);

            //         let len_bytes = compressed.len();

            //         out.write_u8(self.stream_index).unwrap();
            //         out.write_u64::<LittleEndian>(len_bytes as u64).unwrap();
            //         out.write_u64::<LittleEndian>(len_elements as u64).unwrap();

            let stream_index = self.reader.read_u8()?;

            let len_bytes = self.reader.read_u64::<LittleEndian>()?;
            let len_elements = self.reader.read_u64::<LittleEndian>()?;

            let mut compressed_data = vec![0; len_bytes as usize];
            self.reader.read_exact(&mut compressed_data)?;

            let mut data =
                Cursor::new(lz4_flex::decompress_size_prepended(&compressed_data).unwrap());

            let timestamps = delta_decode(&mut data, len_elements as usize).unwrap();
            let locations = delta_decode(&mut data, len_elements as usize).unwrap();

            let entries = timestamps
                .into_iter()
                .zip(locations.into_iter())
                .map(|(ts, location)| SeekEntry { ts, location })
                .collect();
            seektables.push((stream_index, entries));
        }

        Ok((
            Reader {
                start_of_packets: self.reader.stream_position()?,
                reader: self.reader,
                scratch: self.scratch,
                _spooky: PhantomData,
                seektable: seektables[0].1.clone(),
                last_time: 0,
                decoders: self.decoders,
            },
            seektables,
        ))
    }
}

impl<R: Read + Seek> Reader<R, states::SeektablesRead> {
    pub fn seek(&mut self, time: i64) -> std::io::Result<i64> {
        let entry = match self.seektable.binary_search_by_key(&time, |v| v.ts) {
            Ok(idx) => idx,
            Err(idx) => idx,
        };

        let entry = self.seektable[entry];
        self.reader.seek(std::io::SeekFrom::Start(
            entry.location as u64 + self.start_of_packets,
        ))?;

        Ok(entry.ts)
    }

    pub fn read_packet(&mut self) -> std::io::Result<(Packet, Vec<u8>)> {
        let mut packet = Packet::decode_from(&mut self.reader)?;

        let mut data = vec![0u8; packet.data_len as usize];
        self.reader.read_exact(&mut data)?;

        if let Some(decoder) = self.decoders.get_mut(&packet.stream) {
            decoder.process(&mut packet, &mut data)?;
        }

        Ok((packet, data))
    }

    pub fn read_packet_data_into(&mut self, data: &mut Vec<u8>) -> std::io::Result<Packet> {
        let mut packet = Packet::decode_from(&mut self.reader)?;

        let len = packet.data_len as usize;
        data.resize(len, 0);
        self.reader.read_exact(data)?;

        if let Some(decoder) = self.decoders.get_mut(&packet.stream) {
            decoder.process(&mut packet, data)?;
        }

        Ok(packet)
    }

    pub fn read_packet_into_channel(
        &mut self,
        channel: &mpsc::blocking::Sender<PacketWithData, WithCapacity>,
    ) -> std::io::Result<()> {
        let mut packet = Packet::decode_from(&mut self.reader)?;

        let mut send_slot = channel.send_ref().unwrap();

        let len = packet.data_len as usize;
        send_slot.data.resize(len, 0);
        self.reader.read_exact(&mut send_slot.data)?;

        if let Some(decoder) = self.decoders.get_mut(&packet.stream) {
            decoder.process(&mut packet, &mut send_slot.data)?;
        }

        send_slot.header = packet;

        Ok(())
    }
}
