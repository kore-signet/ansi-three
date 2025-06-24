use container::{
    EncodableData, PacketDataType, SubRect, SubRectVec,
    metadata::{FormatData, Stream},
    seek::SeekEntry,
};
use crossterm::{
    execute,
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};
use parking_lot::{Condvar, Mutex};
use spin_sleep::SpinSleeper;
use stable_vec::StableVec;
use std::{
    io::{self, IoSlice, Read, Seek, Write},
    sync::{Arc, atomic::AtomicU8},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use thingbuf::{mpsc::blocking::Receiver, recycling::WithCapacity};

use crate::{FormatDuration, PacketWithData, Reader, states};

pub struct PlayerControl<R: Read + Seek + Send + 'static> {
    pub state: RendererState,
    pause_time: Option<Instant>,

    pub header: FormatData,
    pub video_stream: Stream,

    reader_handle: Arc<Mutex<Reader<R, states::SeektablesRead>>>,

    reader_thread: JoinHandle<()>,
    render_thread: JoinHandle<()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayThreadState {
    Playing,
    Paused,
    DiscardRequest,
    DiscardDone,
}

impl Default for PlayThreadState {
    fn default() -> Self {
        PlayThreadState::Paused
    }
}

pub struct RendererState {
    pub play_status: Arc<(Mutex<PlayThreadState>, Condvar)>,
    pub current_time: Arc<Mutex<Instant>>,
    pub video_time: Arc<Mutex<Duration>>,
    pub subtitle_index: Arc<AtomicU8>,
}

impl Clone for RendererState {
    fn clone(&self) -> Self {
        Self {
            play_status: Arc::clone(&self.play_status),
            current_time: Arc::clone(&self.current_time),
            video_time: Arc::clone(&self.video_time),
            subtitle_index: Arc::clone(&self.subtitle_index),
        }
    }
}

impl<R: Read + Seek + Send + 'static> PlayerControl<R> {
    pub fn new(
        input: R,
        mut output: impl Write + Send + 'static,
    ) -> anyhow::Result<PlayerControl<R>> {
        let input = Reader::new(input);
        let (input, header) = input.read_header()?;
        let (input, seektables) = input.read_seektables()?;

        let input = Arc::new(Mutex::new(input));

        let (packet_tx, packet_rx) = thingbuf::mpsc::blocking::with_recycle::<PacketWithData, _>(
            100,
            WithCapacity::new().with_min_capacity(192 * 108 * 20),
        );

        let input_handle = Arc::clone(&input);
        let reader_thread = std::thread::spawn(move || {
            while let Ok(mut slot) = packet_tx.send_ref() {
                let mut reader_lock = input_handle.lock();
                let packet = reader_lock.read_packet_data_into(&mut slot.data);
                drop(reader_lock);
                slot.header = packet.unwrap();
            }

            while let Ok(()) = input_handle.lock().read_packet_into_channel(&packet_tx) {}

            drop(packet_tx);
        });

        let state = RendererState {
            play_status: Default::default(),
            current_time: Arc::new(Mutex::new(Instant::now())),
            video_time: Default::default(),
            subtitle_index: Arc::new(AtomicU8::new(255)),
        };

        let pause_time = Some(Instant::now());

        let state_handle = state.clone();
        let video_stream = header
            .tracks
            .iter()
            .find(|v: &&Stream| v.parameters.is_video())
            .unwrap()
            .clone();
        // let total_duration = Duration::from_micros(video_stream.duration);
        let video_two = video_stream.clone();
        let render_thread =
            std::thread::spawn(move || render_loop(video_two, output, packet_rx, state_handle));

        Ok(PlayerControl {
            state,
            pause_time,
            video_stream,
            header,
            reader_handle: input,
            reader_thread,
            render_thread,
        })
    }

    pub fn auto_select_subtitles(&self) {
        for stream in &self.header.tracks {
            if stream.parameters.is_subtitle() {
                self.state
                    .subtitle_index
                    .store(stream.index, std::sync::atomic::Ordering::Release);
                break;
            }
        }
    }

    pub fn select_subtitles(&self, index: u8) {
        self.state
            .subtitle_index
            .store(index, std::sync::atomic::Ordering::Release);
    }

    pub fn seek(&mut self, time: Duration) -> io::Result<()> {
        let wait_start = Instant::now();
        let mut reader = self.reader_handle.lock();

        let old_state = *self.state.play_status.0.lock();

        *self.state.play_status.0.lock() = PlayThreadState::DiscardRequest;
        self.state.play_status.1.notify_all();

        let mut current_time = self.state.current_time.lock();
        let video_time = self.state.video_time.lock();

        // if seek forward: reduce current_time by difference, else add
        let actual_time = reader.seek(time.as_micros() as i64)?;

        let delta = video_time.as_micros() as i64 - actual_time;
        if delta >= 0 {
            *current_time += Duration::from_micros(delta as u64);
        } else {
            *current_time -= Duration::from_micros(delta.abs() as u64);
        }

        self.wait_for_state(|v| *v != PlayThreadState::DiscardDone);

        drop(reader);

        *current_time += wait_start.elapsed();

        drop(current_time);
        drop(video_time);

        *self.state.play_status.0.lock() = old_state;
        self.state.play_status.1.notify_all();

        Ok(())
    }

    pub fn seek_forward(&mut self, time: Duration) -> io::Result<()> {
        let target_time = *self.state.video_time.lock() + time;
        self.seek(target_time);

        Ok(())
    }

    pub fn seek_backwards(&mut self, time: Duration) -> io::Result<()> {
        let target_time = (*self.state.video_time.lock())
            .checked_sub(time)
            .unwrap_or_default();
        self.seek(target_time);

        Ok(())
    }

    fn wait_for_state(&self, mut keep_waiting: impl FnMut(&mut PlayThreadState) -> bool) {
        let &(ref lock, ref cvar) = &*self.state.play_status;
        if !keep_waiting(&mut *lock.lock()) {
            return;
        }

        let mut lock = lock.lock();
        cvar.wait_while(&mut lock, keep_waiting);
    }

    pub fn pause(&mut self) {
        if *self.state.play_status.0.lock() == PlayThreadState::Paused {
            return;
        }

        self.pause_time = Some(Instant::now());
        self.wait_for_state(|s| *s != PlayThreadState::Playing);

        *self.state.play_status.0.lock() = PlayThreadState::Paused;
    }

    pub fn resume(&mut self) {
        self.wait_for_state(|s| {
            *s == PlayThreadState::DiscardRequest || *s == PlayThreadState::DiscardDone
        });

        if let Some(t) = self.pause_time {
            *self.state.current_time.lock() += t.elapsed();
        };

        *self.state.play_status.0.lock() = PlayThreadState::Playing;
        self.state.play_status.1.notify_all();
    }

    pub fn join(mut self) {
        self.reader_thread.join();
        self.render_thread.join();
    }
}

// impl Default for Renderer {
//     fn default() -> Self {
//         Self { play_status: Default::default(), current_time: Arc::new(Mutex::new(Instant::now())), pause_time: Some(Instant::now()) }
//     }
// }

struct Subtitle {
    stream: u8,
    starts_at: Duration,
    ends_at: Duration,
    subtitle: String,
}

fn render_loop(
    video_stream: Stream,
    mut output: impl Write + Send + 'static,
    receiver: Receiver<PacketWithData, WithCapacity>,
    state: RendererState,
) {
    output.write_all(b"\x1b[1;1H\x1b[?25l").unwrap();
    output.flush().unwrap();

    let sleeper = SpinSleeper::default();
    let mut subs: StableVec<Subtitle> = StableVec::with_capacity(8);

    let video_params = video_stream.parameters.as_video().unwrap().clone();
    let total_duration = Duration::from_micros(video_stream.duration);

    'play: loop {
        // wait for play status to shift to true
        let &(ref lock, ref cvar) = &*state.play_status;
        let mut playing = lock.lock();
        cvar.wait_while(&mut playing, |v| {
            *v == PlayThreadState::Paused || *v == PlayThreadState::DiscardDone
        });

        let cur_state = *playing;
        drop(playing);

        if cur_state == PlayThreadState::DiscardRequest {
            while let Ok(slot) = receiver.try_recv_ref() {
                if slot.header.data_type == PacketDataType::Subtitle {
                    let new_subs: Vec<SubRect> = SubRectVec::decode_from(&mut slot.data.as_slice())
                        .unwrap()
                        .into_inner();

                    for sub in new_subs {
                        subs.push(Subtitle {
                            stream: slot.header.stream,
                            subtitle: sub.to_string(),
                            starts_at: slot.header.timestamp,
                            ends_at: slot.header.timestamp + slot.header.duration,
                        });
                    }
                }
            }

            *lock.lock() = PlayThreadState::DiscardDone;
            cvar.notify_all();

            continue 'play;
        }

        let Some(slot) = receiver.recv_ref() else {
            break 'play;
        };

        if slot.header.data_type == PacketDataType::Subtitle {
            let new_subs: Vec<SubRect> = SubRectVec::decode_from(&mut slot.data.as_slice())
                .unwrap()
                .into_inner();

            for sub in new_subs {
                subs.push(Subtitle {
                    stream: slot.header.stream,
                    subtitle: sub.to_string(),
                    starts_at: slot.header.timestamp,
                    ends_at: slot.header.timestamp + slot.header.duration,
                });
            }

            continue 'play;
        }

        execute!(output, BeginSynchronizedUpdate).unwrap();

        *state.video_time.lock() = slot.header.timestamp;
        let start = *state.current_time.lock();
        let line = start + slot.header.timestamp - Duration::from_millis(3);

        let mut slices: Vec<IoSlice<'_>> =
            vec![IoSlice::new(b"\x1b[0m\x1b[1;1H"), IoSlice::new(&slot.data)];

        let bar_filled = ((slot.header.timestamp.as_secs_f64() / total_duration.as_secs_f64())
            * video_params.width as f64)
            .round() as usize;

        let time_bar = format!(
            "\x1b[0m\x1b[0;32m{}\x1b[0m{}",
            "■".repeat(bar_filled),
            "■".repeat(video_params.width as usize - bar_filled)
        );

        slices.push(IoSlice::new(time_bar.as_bytes()));

        let time_marker = format!(
            "\x1b[0m\n\r{} | {}",
            FormatDuration(slot.header.timestamp),
            FormatDuration(total_duration)
        );
        slices.push(IoSlice::new(time_marker.as_bytes()));

        subs.retain(|&Subtitle { ends_at, .. }| (start + ends_at) >= line);
        for (
            _,
            &Subtitle {
                ref subtitle,
                starts_at,
                stream,
                ..
            },
        ) in &subs
        {
            if starts_at > slot.header.timestamp + slot.header.duration
                || stream
                    != state
                        .subtitle_index
                        .load(std::sync::atomic::Ordering::Acquire)
            {
                continue;
            }

            slices.push(IoSlice::new(subtitle.as_bytes()));
        }

        slices.push(IoSlice::new(b"\x1b[0m\n"));

        sleeper.sleep_until(line);

        output.write_all_vectored(&mut slices).unwrap();

        execute!(output, EndSynchronizedUpdate).unwrap();

        output.flush().unwrap();
    }
}

// let mut stdout = BufWriter::with_capacity(192 * 108 * 20, std::io::stdout().lock());
// stdout.write_all(b"\x1b[1;1H\x1b[?25l")?;
// stdout.flush()?;

// let sleeper = SpinSleeper::default();
// let start = Instant::now();

// let mut subs: StableVec<(String, Instant)> = StableVec::with_capacity(8);
// while let Some(slot) = packet_rx.recv_ref() {

//     let line = start + slot.header.timestamp - Duration::from_millis(3);

//     let mut slices: Vec<IoSlice<'_>> =
//         vec![IoSlice::new(b"\x1b[0m\x1b[1;1H"), IoSlice::new(&slot.data)];

//     let time_marker = format!("\x1b[0m\n\r{}", FormatDuration(slot.header.timestamp));
//     slices.push(IoSlice::new(time_marker.as_bytes()));

//     subs.retain(|(_, t)| *t > line);
//     for (_, (sub, _)) in &subs {
//         slices.push(IoSlice::new(sub.as_bytes()));
//     }

//     slices.push(IoSlice::new(b"\x1b[0m\n"));

//     // sleeper.sleep(slot.header.duration - Duration::from_millis(3));
//     sleeper.sleep_until(line);

//     // stdout.write_all_vectored(&mut sub_slice)?;
//     // stdout.flush()?;
//     // writeln!(time_log, "prep {:?} render {:?}", prepare_time, write_time);
//     // time_log.flush()?;
// }
