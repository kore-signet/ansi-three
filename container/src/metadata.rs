use std::{fmt::Display, str::FromStr};

use rasn::prelude::*;

#[derive(AsnType, Debug, Clone, Decode, Encode, PartialEq, Eq, Hash)]
#[rasn(choice)]
pub enum CodecParameters {
    #[rasn(tag(explicit(context, 0)))]
    Subtitle(SubtitleParameters),
    #[rasn(tag(explicit(context, 1)))]
    Video(VideoParameters),
}

impl CodecParameters {
    pub fn is_video(&self) -> bool {
        match self {
            CodecParameters::Video(_) => true,
            _ => false,
        }
    }

    pub fn is_subtitle(&self) -> bool {
        match self {
            CodecParameters::Subtitle(_) => true,
            _ => false,
        }
    }

    pub fn as_video(&self) -> Option<&VideoParameters> {
        match self {
            CodecParameters::Video(video_parameters) => Some(video_parameters),
            _ => None,
        }
    }
}

#[derive(AsnType, Debug, Clone, Decode, Encode, PartialEq, Eq, Hash, Copy)]
#[rasn(enumerated)]
pub enum ColorMode {
    Full = 0,
    EightBit = 1,
}

impl FromStr for ColorMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "full" | "rgb" | "true" | "24bit" => ColorMode::Full,
            "eightbit" | "8bit" | "eight" | "256" | "256color" => ColorMode::EightBit,
            _ => return Err("Invalid color mode!"),
        })
    }
}

impl Display for ColorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorMode::Full => write!(f, "full"),
            ColorMode::EightBit => write!(f, "8bit"),
        }
    }
}

#[derive(AsnType, Debug, Clone, Decode, Encode, PartialEq, Eq, Hash, Copy)]
#[rasn(enumerated)]
#[repr(u8)]
pub enum CompressionMode {
    None = 0,
    Zstd = 1,
    Lz4 = 2,
}

impl FromStr for CompressionMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "none" | "null" => CompressionMode::None,
            "zst" | "zstd" => CompressionMode::Zstd,
            "lz4" => CompressionMode::Lz4,
            _ => return Err("Invalid compression mode!"),
        })
    }
}

impl Display for CompressionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CompressionMode::None => "none",
            CompressionMode::Zstd => "zstd",
            CompressionMode::Lz4 => "lz4",
        })
    }
}

#[derive(AsnType, Debug, Clone, Decode, Encode, PartialEq, Eq, Hash)]
pub struct FormatData {
    #[rasn(identifier = "format-name", tag(explicit(context, 0)))]
    pub format_name: Utf8String,
    #[rasn(tag(explicit(context, 1)))]
    pub encoder: Utf8String,
    #[rasn(tag(explicit(context, 2)))]
    pub tracks: SequenceOf<Stream>,
}

impl FormatData {
    pub fn new(format_name: Utf8String, encoder: Utf8String, tracks: SequenceOf<Stream>) -> Self {
        Self {
            format_name,
            encoder,
            tracks,
        }
    }
}

#[derive(AsnType, Debug, Clone, Decode, Encode, PartialEq, Eq, Hash)]
pub struct Stream {
    #[rasn(tag(explicit(context, 0)))]
    pub name: Utf8String,
    #[rasn(value("0..=255"), tag(explicit(context, 1)))]
    pub index: u8,
    #[rasn(tag(explicit(context, 2)))]
    pub duration: u64, // microseconds
    #[rasn(tag(explicit(context, 3)))]
    pub extradata: OctetString,
    #[rasn(identifier = "compression-mode", tag(explicit(context, 4)))]
    pub compression_mode: CompressionMode,
    #[rasn(identifier = "compression-dict", tag(explicit(context, 5)))]
    pub compression_dict: Option<OctetString>,
    #[rasn(tag(explicit(context, 6)))]
    pub parameters: CodecParameters,
}

impl Stream {
    pub fn new(
        name: Utf8String,
        index: u8,
        duration: u64,
        extradata: OctetString,
        compression_dict: Option<OctetString>,
        compression_mode: CompressionMode,
        parameters: CodecParameters,
    ) -> Self {
        Self {
            name,
            index,
            duration,
            extradata,
            compression_mode,
            compression_dict,
            parameters,
        }
    }
}

#[derive(AsnType, Debug, Clone, Decode, Encode, PartialEq, Eq, Hash)]
pub struct SubtitleParameters {
    #[rasn(tag(explicit(context, 0)))]
    pub lang: Utf8String,
    #[rasn(
        value("0..=65535"),
        identifier = "play-width",
        tag(explicit(context, 1))
    )]
    pub play_width: u16,
    #[rasn(
        value("0..=65535"),
        identifier = "play-height",
        tag(explicit(context, 2))
    )]
    pub play_height: u16,
}

impl SubtitleParameters {
    pub fn new(lang: Utf8String, play_width: u16, play_height: u16) -> Self {
        Self {
            lang,
            play_width,
            play_height,
        }
    }
}

#[derive(AsnType, Debug, Clone, Decode, Encode, PartialEq, Eq, Hash)]
pub struct VideoParameters {
    #[rasn(value("0..=65535"), tag(explicit(context, 0)))]
    pub width: u16,
    #[rasn(value("0..=65535"), tag(explicit(context, 1)))]
    pub height: u16,
    #[rasn(tag(explicit(context, 2)))]
    pub color: ColorMode,
}

impl VideoParameters {
    pub fn new(width: u16, height: u16, color: ColorMode) -> Self {
        Self {
            width,
            height,
            color,
        }
    }
}
