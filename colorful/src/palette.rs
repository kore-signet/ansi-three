use super::delta;
use std::marker::PhantomData;

// TODO: configurable palettes
/***
Available constants:
PALETTE: [[u8; 3]; 256]
LAB_PALETTE/JAB_PALETTE: [(f32, f32, f32); 256]
LAB_PALETTE_FLATTENED/JAB_PALETTE_FLATTENED: [f32; 1024] layout = [l/j, a, b, 0.0] * 256
PALETTE_FG_CODES: [&'static str; 256]
PALETTE_BG

***/

mod const_gen {
    include!(concat!(env!("OUT_DIR"), "/const_gen.rs"));
}

/// [(f32, f32, f32); 256]
pub use const_gen::JAB_PALETTE;
/// [f32; 1024]
pub use const_gen::JAB_PALETTE_FLATTENED;
/// [(f32, f32, f32); 256]
pub use const_gen::LAB_PALETTE;
/// [f32; 1024]
pub use const_gen::LAB_PALETTE_FLATTENED;
/// [[u8; 3]; 256]
pub use const_gen::PALETTE;
/// [&'static str; 256]
pub use const_gen::PALETTE_BG_CODES;
/// [&'static str; 256]
pub use const_gen::PALETTE_FG_CODES;
/// phf::Map<[u8; 3], u8>
pub use const_gen::REVERSE_PALETTE;
/// phf::Map<[u8; 3], &'static str>
pub use const_gen::REVERSE_PALETTE_BG_CODES;
/// phf::Map<[u8; 3], &'static str>
pub use const_gen::REVERSE_PALETTE_FG_CODES;

use image::{imageops::ColorMap, Rgb};

pub trait DistanceMethod {
    fn closest(color: &[u8; 3]) -> usize;
}

macro_rules! distance_method {
    ($name:ident : $func:path) => {
        #[derive(Copy, Clone, Debug)]
        pub struct $name;

        impl DistanceMethod for $name {
            #[inline(always)]
            fn closest(color: &[u8; 3]) -> usize {
                $func(color).0 as usize
            }
        }
    };
}

distance_method!(CAM02: delta::jab::closest_ansi);
distance_method!(CIE94: delta::cie94::closest_ansi);
distance_method!(CIE76: delta::cie76::closest_ansi);

#[derive(Clone, Copy, Debug)]
pub struct AnsiColorMap<T: DistanceMethod> {
    _spooky: PhantomData<T>,
}

impl<T: DistanceMethod> Default for AnsiColorMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: DistanceMethod> AnsiColorMap<T> {
    pub const fn new() -> AnsiColorMap<T> {
        AnsiColorMap {
            _spooky: PhantomData,
        }
    }

    pub fn reverse_lookup(color: &[u8; 3]) -> Option<u8> {
        REVERSE_PALETTE.get(color).copied()
    }
}

impl<T: DistanceMethod> ColorMap for AnsiColorMap<T> {
    type Color = Rgb<u8>;

    #[inline(always)]
    fn index_of(&self, color: &Rgb<u8>) -> usize {
        T::closest(&color.0)
    }

    #[inline(always)]
    fn lookup(&self, idx: usize) -> Option<Self::Color> {
        Some(Rgb(PALETTE[idx]))
    }

    #[inline(always)]
    fn has_lookup(&self) -> bool {
        true
    }

    #[inline(always)]
    fn map_color(&self, color: &mut Rgb<u8>) {
        *color = self.lookup(self.index_of(color)).unwrap();
    }
}
