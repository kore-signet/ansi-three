use std::time::Duration;

use container::SubRect;
use ffmpeg_the_third::{Packet, Stream, frame::Video as VideoFrame, media::Type as StreamType};
use thingbuf::{Recycle, recycling};

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PacketType {
    Video,
    Subtitle,
    Unknown,
    Invalid,
}

pub struct FFPacket {
    pub stream_idx: usize,
    pub frame_idx: usize, // within the stream
    pub kind: PacketType,
    pub timestamp: Duration,
    pub duration: Duration,
    pub binary_data: Vec<u8>,
    pub sub_rects: Vec<SubRect>,
}

impl FFPacket {
    pub fn ingest_video(
        &mut self,
        stream: &Stream<'_>,
        idx: usize,
        pts: u64,
        duration: u64,
        packet: &VideoFrame,
    ) {
        self.stream_idx = stream.index();
        self.frame_idx = idx;
        self.kind = PacketType::Video;
        self.timestamp = Duration::from_micros(pts);
        self.duration = Duration::from_micros(duration);
        self.binary_data.extend_from_slice(packet.data(0));
    }

    pub fn ingest_packet(
        &mut self,
        stream: &Stream<'_>,
        idx: usize,
        with_data: bool,
        packet: &Packet,
    ) {
        self.stream_idx = stream.index();
        self.frame_idx = idx;
        self.kind = match stream.parameters().medium() {
            StreamType::Video => PacketType::Video,
            StreamType::Subtitle => PacketType::Subtitle,
            _ => PacketType::Unknown,
        };
        self.duration = Duration::from_micros(packet.duration() as u64);
        self.timestamp = Duration::from_micros(packet.pts().unwrap() as u64);

        if with_data && let Some(data) = packet.data() {
            self.binary_data.extend_from_slice(data);
        }
    }
}

impl Default for FFPacket {
    fn default() -> Self {
        Self {
            stream_idx: usize::MAX,
            frame_idx: usize::MAX,
            kind: PacketType::Invalid,
            timestamp: Default::default(),
            duration: Default::default(),
            binary_data: Vec::new(),
            sub_rects: Vec::new(),
        }
    }
}

impl Recycle<FFPacket> for recycling::WithCapacity {
    fn new_element(&self) -> FFPacket {
        FFPacket {
            binary_data: Vec::with_capacity(self.min_capacity()),
            ..Default::default()
        }
    }

    fn recycle(&self, element: &mut FFPacket) {
        let mut data_buffer = std::mem::take(&mut element.binary_data);
        data_buffer.clear();
        data_buffer.shrink_to(self.max_capacity());
        *element = FFPacket {
            binary_data: data_buffer,
            ..Default::default()
        };
    }
}
