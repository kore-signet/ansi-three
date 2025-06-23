//! Thomas Knoll dithering algorithm, based on https://bisqwit.iki.fi/story/howto/dither/jy/#PatternDitheringThePatentedAlgorithmUsedInAdobePhotoshop

use std::fmt::Display;
use std::str::FromStr;

use crate::palette::{AnsiColorMap, DistanceMethod, PALETTE};
use arrayvec::ArrayVec;
use image::imageops::ColorMap;
use image::{GenericImageView, ImageBuffer, Luma, Rgb};
use num_enum::TryFromPrimitive;
use rayon::prelude::*;

static BAYER_8X8: [usize; 64] = [
    0, 48, 12, 60, 3, 51, 15, 63, 32, 16, 44, 28, 35, 19, 47, 31, 8, 56, 4, 52, 11, 59, 7, 55, 40,
    24, 36, 20, 43, 27, 39, 23, 2, 50, 14, 62, 1, 49, 13, 61, 34, 18, 46, 30, 33, 17, 45, 29, 10,
    58, 6, 54, 9, 57, 5, 53, 42, 26, 38, 22, 41, 25, 37, 21,
];
static BAYER_4X4: [usize; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
static BAYER_2X2: [usize; 4] = [0, 2, 3, 1];

#[derive(TryFromPrimitive, Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum MatrixSize {
    Eight = 0,
    Four = 1,
    Two = 2,
}

impl Display for MatrixSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatrixSize::Eight => write!(f, "two"),
            MatrixSize::Four => write!(f, "four"),
            MatrixSize::Two => write!(f, "eight"),
        }
    }
}

impl FromStr for MatrixSize {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "two" | "2" | "2x2" => MatrixSize::Two,
            "four" | "4" | "4x4" => MatrixSize::Four,
            "eight" | "8" | "8x8" => MatrixSize::Eight,
            _ => return Err("Invalid matrix size!"),
        })
    }
}

fn to_luma(c: [u8; 3]) -> f32 {
    (c[0] as f32 * 299.0 + c[1] as f32 * 587.0 + c[2] as f32 * 114.0) / 255000.0
}

macro_rules! mix_def {
    ($name:ident, $size:literal) => {
        pub fn $name(
            color: [u8; 3],
            multiplier: f32,
            color_map: AnsiColorMap<impl DistanceMethod>,
            pick_idx: usize,
        ) -> u8 {
            let mut err_acc: [u8; 3] = [0, 0, 0];
            // let mut candidates: Vec<[u8; 3]> = Vec::with_capacity(size);
            let mut candidates: ArrayVec<([u8; 3], u8), $size> = ArrayVec::new();

            for _ in 0..$size {
                let tmp = [
                    (color[0] as f32 + (err_acc[0] as f32 * multiplier)).clamp(0.0, 255.0) as u8,
                    (color[1] as f32 + (err_acc[1] as f32 * multiplier)).clamp(0.0, 255.0) as u8,
                    (color[2] as f32 + (err_acc[2] as f32 * multiplier)).clamp(0.0, 255.0) as u8,
                ];

                let chosen = color_map.index_of(&Rgb(tmp));

                let chosen_c = PALETTE[chosen];
                candidates.push((chosen_c, chosen as u8));

                err_acc[0] = err_acc[0].saturating_add(color[0].saturating_sub(chosen_c[0]));
                err_acc[1] = err_acc[1].saturating_add(color[1].saturating_sub(chosen_c[1]));
                err_acc[2] = err_acc[2].saturating_add(color[2].saturating_sub(chosen_c[2]));
            }

            candidates.sort_by(|a, b| to_luma(a.0).partial_cmp(&to_luma(b.0)).unwrap());

            candidates[pick_idx].1
        }
    };
}

mix_def!(mix_2x2, 4);
mix_def!(mix_4x4, 16);
mix_def!(mix_8x8, 64);

pub fn dither(
    image: &(impl GenericImageView<Pixel = Rgb<u8>> + Sync + Send),
    matrix_size: MatrixSize,
    multiplier: f32,
    color_map: AnsiColorMap<impl DistanceMethod + Send + Sync + Copy>,
) -> Vec<u8> {
    let height = image.height() as usize;
    let width = image.width() as usize;
    let mut out: Vec<u8> = vec![0; width * height];

    match matrix_size {
        MatrixSize::Two => out.par_iter_mut().enumerate().for_each(|(i, pixel_out)| {
            let (x, y) = (i % width, i / width);
            let pixel = image.get_pixel(x as u32, y as u32);
            *pixel_out = mix_2x2(
                pixel.0,
                multiplier,
                color_map,
                BAYER_2X2[(y % 2) * 2 + (x % 2)],
            );
        }),
        MatrixSize::Four => out.par_iter_mut().enumerate().for_each(|(i, pixel_out)| {
            let (x, y) = (i % width, i / width);
            let pixel = image.get_pixel(x as u32, y as u32);
            *pixel_out = mix_4x4(
                pixel.0,
                multiplier,
                color_map,
                BAYER_4X4[(y % 4) * 4 + (x % 4)],
            );
        }),
        MatrixSize::Eight => out.par_iter_mut().enumerate().for_each(|(i, pixel_out)| {
            let (x, y) = (i % width, i / width);
            let pixel = image.get_pixel(x as u32, y as u32);
            *pixel_out = mix_8x8(
                pixel.0,
                multiplier,
                color_map,
                BAYER_8X8[(y % 8) * 8 + (x % 8)],
            );
        }),
    };

    out
}

pub trait PatternDither: GenericImageView<Pixel = Rgb<u8>> + Sync + Send + Sized {
    fn pattern_dither(
        &self,
        matrix_size: MatrixSize,
        multiplier: f32,
        color_map: AnsiColorMap<impl DistanceMethod + Send + Sync + Copy>,
    ) -> ImageBuffer<Luma<u8>, Vec<u8>> {
        let dithered = dither(self, matrix_size, multiplier, color_map);
        ImageBuffer::from_vec(self.width(), self.height(), dithered).unwrap()
    }
}

impl<T> PatternDither for T where T: GenericImageView<Pixel = Rgb<u8>> + Sync + Send + Sized {}
