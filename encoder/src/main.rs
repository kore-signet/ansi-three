use std::{
    fs::File,
    io::{BufReader, BufWriter, Seek, Write},
    ops::Deref,
    path::PathBuf,
    str::FromStr,
};

use byteorder::{LittleEndian, WriteBytesExt};
use clap::{
    Parser,
    builder::{PossibleValuesParser, TypedValueParser},
};
use colorful::pattern_dithering::MatrixSize;
use container::{
    EncodableData, FormatDuration, Packet,
    metadata::{ColorMode, CompressionMode, SubtitleParameters, VideoParameters},
};
use encoder::{
    encoders::{
        Pipeline, SeekTableEncoder,
        subtitles::AnsiSubtitleEncoder,
        video::{AnsiVideoEncoder, DitherMethod},
    },
    ff::{self},
};
use encoder::{
    encoders::{lz4::Lz4Compressor, zstd::ZstdCompressor},
    ff::decoder::FFDecoder,
};
use litemap::LiteMap;
use rasn::types::OctetString;

#[derive(clap::Parser, Debug)]
#[command()]
pub struct EncoderArgs {
    #[arg(short, long, value_name = "FILE")]
    input: String,
    #[arg(short, long, value_name = "FILE")]
    output: PathBuf,
    #[arg(long, default_value_t = ColorMode::Full, value_parser = PossibleValuesParser::new(["full", "8bit"]).try_map(|v| ColorMode::from_str(&v)))]
    color_mode: ColorMode,
    /// How to dither this image!
    #[arg(long, value_enum, default_value_t = DitherMethod::FloydSteinberg)]
    dither_method: DitherMethod,
    /// What matrix size to use for pattern dithering (higher = higher quality)
    #[arg(long, default_value_t = MatrixSize::Eight, value_parser = PossibleValuesParser::new(["two", "four", "eight"]).try_map(|v| MatrixSize::from_str(&v)))]
    matrix_size: MatrixSize,
    /// Error multiplier for pattern dithering
    #[arg(long, default_value_t = 0.09)]
    multiplier: f32,
    #[arg(long, default_value_t = 192)]
    width: i64,
    #[arg(long, default_value_t = 108)]
    height: i64,
    #[arg(long)]
    video_dict: Option<PathBuf>,
    #[arg(long, default_value_t = CompressionMode::Lz4, value_parser = PossibleValuesParser::new(["none", "zstd", "lz4"]).try_map(|v| CompressionMode::from_str(&v)))]
    compression_mode: CompressionMode,
}

#[allow(dead_code)]
pub struct ANSIEncoder {
    out: BufWriter<File>,
    scratch: Vec<u8>,
    stream_packet_idx: LiteMap<u8, u64>,
    encoders: LiteMap<u8, Pipeline>,
    width: i64,
    height: i64,                  // seek_table:
    seek_table: SeekTableEncoder, // every n milliseconds, record a seektable entry
    bytes_written: u64,
}

impl ANSIEncoder {
    pub fn new(out: BufWriter<File>, args: &EncoderArgs) -> Self {
        Self {
            out,
            scratch: Vec::with_capacity(args.width as usize * args.height as usize * 20),
            stream_packet_idx: LiteMap::new(),
            encoders: LiteMap::new(),
            width: args.width,
            height: args.height,
            seek_table: SeekTableEncoder::new(0),
            bytes_written: 0,
        }
    }

    fn add_encoder(&mut self, stream: u8, pipeline: Pipeline) {
        self.encoders.insert(stream, pipeline);
    }

    fn process_packet(&mut self, input: &encoder::ff::packet::FFPacket) -> std::io::Result<()> {
        let mut packet = Packet::builder()
            .timestamp(input.timestamp)
            .duration(input.duration)
            .stream(input.stream_idx as u8)
            .build();

        let Some(encoder) = self.encoders.get_mut(&packet.stream) else {
            return Ok(());
        };

        self.scratch.clear();
        encoder.run(input, &mut packet, &mut self.scratch)?;

        let index = self.stream_packet_idx.entry(packet.stream).or_insert(1);
        packet.packet_idx = *index;
        *index += 1;

        self.seek_table.ingest(&packet, self.bytes_written);

        self.bytes_written += packet.encode_into(&mut self.out)?;
        self.out.write_all(&self.scratch)?;
        self.bytes_written += self.scratch.len() as u64;

        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    ff::init()?;

    let cli = EncoderArgs::parse();

    let (ff_decoder, rx) = FFDecoder::new(&cli.input, cli.width, cli.height, |subs| {
        subs.best(ffmpeg_the_third::media::Type::Subtitle)
    })?;

    let mut ansi_encoder = ANSIEncoder::new(
        BufWriter::new(tempfile::tempfile_in(std::env::current_dir()?)?),
        &cli,
    );

    // let dict = std::fs::read("full-color-anime.zstdict")?;

    let mut streams = vec![];

    let video_stream_idx = ff_decoder.video_stream_idx();
    ansi_encoder
        .seek_table
        .set_stream_index(video_stream_idx as u8);

    streams.push(container::metadata::Stream {
        name: "video".to_string(),
        index: ff_decoder.video_stream_idx() as u8,
        duration: ff_decoder.duration().as_micros() as u64,
        extradata: OctetString::default(),
        compression_dict: None,
        parameters: container::metadata::CodecParameters::Video(VideoParameters {
            width: cli.width as u16,
            height: cli.height as u16,
            color: cli.color_mode,
        }),
        compression_mode: CompressionMode::Zstd,
    });

    ansi_encoder.add_encoder(
        ff_decoder.video_stream_idx() as u8,
        Pipeline::new(AnsiVideoEncoder {
            color_mode: cli.color_mode,
            dither_mode: cli.dither_method,
            matrix_size: cli.matrix_size,
            multiplier: cli.multiplier,
            width: cli.width,
            height: cli.height,
        })
        .with_step(ZstdCompressor::new(8)?), // .with_step(ZstdCompressor::with_dict(3, dict)?),
    );

    for subtitle_track in ff_decoder.subs.values() {
        streams.push(container::metadata::Stream {
            name: subtitle_track
                .metadata()
                .get("title")
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_string()),
            index: subtitle_track.stream_index() as u8,
            duration: ff_decoder.duration().as_micros() as u64,
            extradata: OctetString::default(),
            compression_dict: None,
            parameters: container::metadata::CodecParameters::Subtitle(SubtitleParameters {
                lang: subtitle_track
                    .metadata()
                    .get("language")
                    .cloned()
                    .unwrap_or_else(|| "<unknown>".to_string()),
                play_width: cli.width as u16,
                play_height: cli.height as u16,
            }),
            compression_mode: CompressionMode::Lz4,
        });

        ansi_encoder.add_encoder(
            subtitle_track.stream_index() as u8,
            Pipeline::new(AnsiSubtitleEncoder).with_step(Lz4Compressor::default()),
        );
    }

    let format_data = container::metadata::FormatData {
        format_name: "ansi.moe v3.0 (codename yachi-yo!)".to_string(),
        encoder: "ansi.moe ref encoder".to_string(),
        tracks: streams,
    };

    let total_duration = FormatDuration(ff_decoder.duration());

    let receiver = std::thread::spawn(move || -> anyhow::Result<()> {
        while let Some(slot) = rx.recv_ref() {
            let time = slot.timestamp;

            let pct = time.as_secs_f64() / total_duration.0.as_secs_f64();
            print!("\x1b[2K\r");
            print!(
                "{}/{} ({:.2}%)",
                FormatDuration(time),
                total_duration,
                pct * 100.0
            );
            std::io::stdout().flush().unwrap();

            // slot.timestamp

            ansi_encoder.process_packet(slot.deref())?;
        }

        let mut packets_file = ansi_encoder.out.into_inner().unwrap();
        packets_file.seek(std::io::SeekFrom::Start(0))?;

        // finalization
        let mut final_out = BufWriter::new(File::create(cli.output)?);
        let header = rasn::der::encode(&format_data).unwrap();
        final_out.write_u64::<LittleEndian>(header.len() as u64)?;
        final_out.write_all(&header)?;
        // final_out.write_all(&rasn::der::encode(&format_data).unwrap())?;

        let seek_video_table = ansi_encoder.seek_table.finish();
        final_out.write_u8(1)?; // one seek table
        final_out.write_all(&seek_video_table)?;

        std::io::copy(&mut BufReader::new(packets_file), &mut final_out)?;
        //         -- (marker: len_bytes, u64) Header: DER-encoded FormatData
        // -- (marker: len_bytes, u64) Seek Tables
        //     -- (stream_index: u16)
        //     -- (seek_table_length: u64 / bytes)
        // -- (interleaved packet data)

        // out.write_all()

        Ok(())
    });

    ff_decoder.run();
    receiver.join();

    Ok(())
}
