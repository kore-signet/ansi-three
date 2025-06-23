pub use ffmpeg_the_third as ffmpeg;
use ffmpeg_the_third::Rational;

pub use ffmpeg::init;

pub const MICROSECOND_TIMEBASE: Rational = Rational(1, 1_000_000);
pub mod decoder;
pub mod packet;
pub mod subtitles;
