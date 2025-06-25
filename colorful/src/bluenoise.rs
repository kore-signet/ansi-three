use std::ops::Deref;

use image::{imageops::ColorMap, ImageBuffer, Luma, Rgb};
use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator, ParallelIterator};

use crate::palette::{AnsiColorMap, CAM02};

pub struct Bluenoise {
    matrix: ImageBuffer<Luma<u8>, Vec<u8>>,
    range: f64,
}

impl Bluenoise {
    pub fn new(matrix: ImageBuffer<Luma<u8>, Vec<u8>>, range: f64) -> Self {
        Bluenoise { matrix, range }
    }

    pub fn dither(
        &self,
        input: &ImageBuffer<Rgb<u8>, impl Deref<Target = [u8]> + Send + Sync>,
    ) -> ImageBuffer<Luma<u8>, Vec<u8>> {
        let height = input.height() as usize;
        let width = input.width() as usize;
        let mut out: Vec<u8> = vec![0; width * height];
        out.par_iter_mut().enumerate().for_each(|(i, pixel_out)| {
            let (x, y) = (i % width, i / width);
            let mut pixel = *input.get_pixel(x as u32, y as u32);
            let noise = self.matrix.get_pixel(x as u32, y as u32).0[0] as f64 / 255.0f64;
            pixel.0[0] = (pixel.0[0] as f64 + self.range * (noise - 0.5))
                .round()
                .min(255.0) as u8;
            pixel.0[1] = (pixel.0[1] as f64 + self.range * (noise - 0.5))
                .round()
                .min(255.0) as u8;
            pixel.0[2] = (pixel.0[2] as f64 + self.range * (noise - 0.5))
                .round()
                .min(255.0) as u8;

            *pixel_out = (const { AnsiColorMap::<CAM02>::new() }).index_of(&pixel) as u8;
        });

        ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(width as u32, height as u32, out).unwrap()
    }
}
