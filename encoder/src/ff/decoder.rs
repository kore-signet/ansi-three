use std::collections::HashMap;
use std::time::Duration;

use ffmpeg::format::context::common::StreamIter;
use ffmpeg::format::{Pixel, input as ff_input};
use ffmpeg_the_third::codec::Id as CodecID;
use ffmpeg_the_third::codec::context::Context as CodecContext;
use ffmpeg_the_third::codec::decoder::Video as VideoDecoder;
use ffmpeg_the_third::codec::decoder::subtitle::Subtitle as FFSubtitleDecoder;
use ffmpeg_the_third::codec::subtitle::Subtitle as FFSubtitleFrame;
use ffmpeg_the_third::ffi::{AV_TIME_BASE, av_frame_unref};
use ffmpeg_the_third::format::context::Input as InputContext;
use ffmpeg_the_third::software::scaling::{Context as ScalerContext, flag::Flags as ScalerFlags};
use ffmpeg_the_third::util::frame::Video as VideoFrame;
use ffmpeg_the_third::{self as ffmpeg, media::Type as StreamType};
use ffmpeg_the_third::{Rational, Rescale, Stream};
use litemap::LiteMap;
use thingbuf::{mpsc::blocking as channel, recycling::WithCapacity}; // this is gory man

use super::MICROSECOND_TIMEBASE;
use super::packet::FFPacket;
use super::subtitles::{ASSDecoder, SubtitleDecoder};

struct DecoderScratch {
    decoded: VideoFrame,
    scaled: VideoFrame,
}

impl Default for DecoderScratch {
    fn default() -> Self {
        Self {
            decoded: VideoFrame::empty(),
            scaled: VideoFrame::empty(),
        }
    }
}

impl DecoderScratch {
    pub fn get(&mut self) -> (&mut VideoFrame, &mut VideoFrame) {
        (&mut self.decoded, &mut self.scaled)
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        unsafe { av_frame_unref(self.scaled.as_mut_ptr()) };
        unsafe { av_frame_unref(self.decoded.as_mut_ptr()) };
    }
}

pub struct FFDecoder {
    input_ctx: Option<InputContext>,
    video: VideoProcessor,
    pub subs: LiteMap<usize, SubtitleProcessor>,
    packet_tx: channel::Sender<FFPacket, WithCapacity>,
}

pub struct SubtitleProcessor {
    metadata: HashMap<String, String>,
    ff: FFSubtitleDecoder,
    transformer: Box<dyn SubtitleDecoder>,
    sub_index: usize,
    frame_index: usize,
}

impl SubtitleProcessor {
    pub fn stream_index(&self) -> usize {
        self.sub_index
    }

    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    fn from_stream(sub_stream: Stream<'_>, target_x: i64, target_y: i64) -> anyhow::Result<Self> {
        let sub_index = sub_stream.index();
        let metadata = sub_stream
            .metadata()
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();

        let sub_decoder_context = CodecContext::from_parameters(sub_stream.parameters())?;

        let sub_data = 'block: {
            let subtitle_codec = unsafe { sub_decoder_context.as_ptr().as_ref().unwrap() };

            if subtitle_codec.extradata_size <= 0 {
                break 'block String::new();
            }

            let mut data_buf = vec![0; subtitle_codec.extradata_size as usize];
            unsafe {
                std::ptr::copy(
                    subtitle_codec.extradata,
                    data_buf.as_mut_ptr(),
                    subtitle_codec.extradata_size as usize,
                )
            };

            String::from_utf8_lossy(&data_buf).into_owned()
        };

        let ssa_decoder = ASSDecoder::create(&sub_data, target_x, target_y);
        let sub_decoder = sub_decoder_context.decoder().subtitle()?;

        Ok(Self {
            ff: sub_decoder,
            transformer: Box::new(ssa_decoder),
            metadata,
            sub_index,
            frame_index: 0,
        })
    }

    fn process_packet(
        &mut self,
        stream: &Stream<'_>,
        packet: &ffmpeg::Packet,
        tx: &channel::Sender<FFPacket, WithCapacity>,
    ) -> anyhow::Result<()> {
        let mut slot = tx.send_ref()?;
        slot.ingest_packet(stream, self.frame_index, false, packet);

        let mut out = FFSubtitleFrame::new();
        let _ = self.ff.decode(packet, &mut out)?;

        slot.sub_rects = self.transformer.decode_subtitle(&out);

        self.frame_index += 1;

        Ok(())
    }

    fn can_process(&self, idx: usize) -> bool {
        idx == self.sub_index
    }
}

struct VideoProcessor {
    video_stream_idx: usize,
    decoder: VideoDecoder,
    scaler: ScalerContext,
    frame_index: usize,
    scratch: DecoderScratch,
}

impl VideoProcessor {
    fn from_stream(video_stream: Stream<'_>, target_x: i64, target_y: i64) -> anyhow::Result<Self> {
        let index = video_stream.index();

        let mut decoder_ctx = CodecContext::from_parameters(video_stream.parameters())?;
        if let Ok(parallelism) = std::thread::available_parallelism() {
            decoder_ctx.set_threading(ffmpeg::threading::Config {
                kind: ffmpeg::threading::Type::Frame,
                count: parallelism.get(),
            });
        }

        let decoder = decoder_ctx.decoder().video()?;
        let scaler = ScalerContext::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGB24,
            target_x as u32,
            target_y as u32,
            ScalerFlags::BILINEAR,
        )?;

        Ok(VideoProcessor {
            video_stream_idx: index,
            decoder,
            scaler,
            frame_index: 0,
            scratch: DecoderScratch::default(),
        })
    }

    fn decode_videoframes(
        &mut self,
        stream: &Stream<'_>,
        tx: &channel::Sender<FFPacket, WithCapacity>,
    ) -> anyhow::Result<u64> {
        let (decode_buf, scaled_buf) = self.scratch.get();

        let mut decoded = 0;

        while self.decoder.receive_frame(decode_buf).is_ok() {
            self.scaler.run(decode_buf, scaled_buf)?;

            let mut packet_slot = tx.send_ref()?;
            packet_slot.ingest_video(
                stream,
                self.frame_index,
                decode_buf.pts().unwrap() as u64,
                decode_buf.packet().duration as u64,
                scaled_buf,
            );
            self.frame_index += 1;
            decoded += 1;
        }

        Ok(decoded)
    }

    fn can_process(&self, idx: usize) -> bool {
        idx == self.video_stream_idx
    }
}

impl FFDecoder {
    pub fn new(
        path: &str,
        target_x: i64,
        target_y: i64,
        select_subs: impl FnOnce(StreamIter<'_>) -> Option<Stream<'_>>,
    ) -> anyhow::Result<(Self, channel::Receiver<FFPacket, WithCapacity>)> {
        let input_ctx = ff_input(path)?;
        let video_stream = input_ctx
            .streams()
            .best(StreamType::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;

        let subs = input_ctx
            .streams()
            .filter(|s| {
                s.parameters().medium() == ffmpeg::media::Type::Subtitle
                    && [CodecID::ASS, CodecID::SSA].contains(&s.parameters().id())
            })
            .filter_map(|s| SubtitleProcessor::from_stream(s, target_x, target_y).ok())
            .map(|s| (s.sub_index, s))
            .collect();

        let video = VideoProcessor::from_stream(video_stream, target_x, target_y)?;

        let (tx, rx) = channel::with_recycle(
            192,
            WithCapacity::new()
                .with_min_capacity(target_x as usize * target_y as usize * 3)
                .with_max_capacity(target_x as usize * target_y as usize * 4),
        );

        Ok((
            FFDecoder {
                input_ctx: Some(input_ctx),
                video,
                subs,
                packet_tx: tx,
            },
            rx,
        ))
    }

    pub fn video_stream_idx(&self) -> usize {
        self.video.video_stream_idx
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let mut input_ctx = self.input_ctx.take().unwrap();
        for (stream, mut packet) in input_ctx.packets().filter_map(Result::ok) {
            packet.rescale_ts(stream.time_base(), MICROSECOND_TIMEBASE);

            if self.video.can_process(stream.index()) {
                self.video.decoder.send_packet(&packet)?;
                let _ = self.video.decode_videoframes(&stream, &self.packet_tx)?;

                continue;
            }

            if let Some(processor) = self.subs.get_mut(&stream.index()) {
                processor.process_packet(&stream, &packet, &self.packet_tx)?;
            }
        }

        self.video.decoder.send_eof()?;
        self.video.decode_videoframes(
            &input_ctx.stream(self.video.video_stream_idx).unwrap(),
            &self.packet_tx,
        )?;

        Ok(())
    }

    pub fn duration(&self) -> Duration {
        Duration::from_micros(
            self.input_ctx
                .as_ref()
                .unwrap()
                .duration()
                .rescale(Rational::new(1, AV_TIME_BASE), MICROSECOND_TIMEBASE) as u64,
        )
    }
}
