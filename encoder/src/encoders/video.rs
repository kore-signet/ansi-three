use clap::ValueEnum;
use colorful::{
    palette::{AnsiColorMap, CAM02},
    pattern_dithering::{MatrixSize, PatternDither},
};
use container::{EncodableData, Packet as AnsiPacket, PacketDataType, metadata::ColorMode};
use image::{ImageBuffer, Rgb, imageops};
use img2ansi::AnsiFrame;

use crate::{encoders::FFToAnsi, ff::packet::FFPacket};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum DitherMethod {
    FloydSteinberg,
    Pattern,
}

pub struct AnsiVideoEncoder {
    pub color_mode: ColorMode,
    pub dither_mode: DitherMethod,
    pub matrix_size: MatrixSize,
    pub multiplier: f32,
    pub width: i64,
    pub height: i64,
}

impl FFToAnsi for AnsiVideoEncoder {
    fn process(
        &mut self,
        input: &FFPacket,
        packet: &mut AnsiPacket,
        data: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        let image = ImageBuffer::<Rgb<u8>, _>::from_raw(
            self.width as u32,
            self.height as u32,
            input.binary_data.as_slice(),
        )
        .unwrap();

        data.reserve((self.width * self.height * 20) as usize);

        match self.color_mode {
            ColorMode::Full => {
                AnsiFrame::from(image).encode_into(data)?;
            }
            ColorMode::EightBit => match self.dither_mode {
                DitherMethod::FloydSteinberg => self.floyd_steinberg(image, data)?,
                DitherMethod::Pattern => self.pattern_dither(image, data)?,
            },
        };

        packet.data_len = data.len() as u64;
        packet.data_type = PacketDataType::Video;

        Ok(())
    }
}

impl AnsiVideoEncoder {
    fn floyd_steinberg(
        &mut self,
        in_image: ImageBuffer<Rgb<u8>, &[u8]>,
        data: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        let mut base_image =
            ImageBuffer::from_vec(self.width as u32, self.height as u32, in_image.to_vec())
                .unwrap();

        let mut indexed_image = ImageBuffer::new(self.width as u32, self.height as u32);
        imageops::dither(&mut base_image, &const { AnsiColorMap::<CAM02>::new() });

        for (pixel, idx) in base_image.pixels().zip(indexed_image.pixels_mut()) {
            *idx = image::Luma([(AnsiColorMap::<CAM02>::reverse_lookup(&pixel.0)).unwrap()]);
        }

        AnsiFrame::from(indexed_image).encode_into(data)?;

        Ok(())
    }

    fn pattern_dither(
        &mut self,
        in_image: ImageBuffer<Rgb<u8>, &[u8]>,
        data: &mut Vec<u8>,
    ) -> std::io::Result<()> {
        let indexed = in_image.pattern_dither(
            self.matrix_size,
            self.multiplier,
            const { AnsiColorMap::<CAM02>::new() },
        );

        AnsiFrame::from(indexed).encode_into(data)?;

        Ok(())
    }
}
