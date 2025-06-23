use container::{EncodableData, PacketDataType, SubRectVec};

use crate::encoders::FFToAnsi;

pub struct AnsiSubtitleEncoder;

impl FFToAnsi for AnsiSubtitleEncoder {
    fn process(
        &mut self,
        input: &crate::ff::packet::FFPacket,
        packet: &mut container::Packet,
        data: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        let subs = SubRectVec::from(input.sub_rects.clone());

        if let Some(est_size) = subs.estimated_size() {
            data.reserve(est_size);
        }

        subs.encode_into(data)?;

        packet.data_len = data.len() as u64;
        packet.data_type = PacketDataType::Subtitle;

        Ok(())
    }
}
