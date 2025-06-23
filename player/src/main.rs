use container::{EncodableData, FormatDuration, PacketDataType, SubRect, SubRectVec};
use crossterm::{
    event::{
        EnableBracketedPaste, EnableFocusChange, EnableMouseCapture, Event, KeyCode,
        KeyboardEnhancementFlags, MouseButton, MouseEventKind, PushKeyboardEnhancementFlags, read,
    },
    execute, queue,
    terminal::{Clear, enable_raw_mode},
};
use player::{PacketWithData, Reader, renderer::PlayerControl};
use spin_sleep::SpinSleeper;
use stable_vec::StableVec;
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, IoSlice, Write},
    time::{Duration, Instant},
};
use termion::{event::Key, input::TermRead, raw::IntoRawMode};
use thingbuf::{mpsc::blocking as mpsc, recycling::WithCapacity};

fn main() -> anyhow::Result<()> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnableBracketedPaste,
        EnableFocusChange,
        EnableMouseCapture,
        Clear(crossterm::terminal::ClearType::All)
    )?;

    let supports_keyboard_enhancement = matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    );

    if supports_keyboard_enhancement {
        queue!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )?;
    }

    stdout.flush()?;

    let mut stdout = BufWriter::with_capacity(192 * 108 * 20, stdout);
    stdout.write_all(b"\x1b[1;1H\x1b[?25l")?;
    stdout.flush()?;

    let mut renderer = PlayerControl::new(BufReader::new(File::open("out.ansi")?), stdout)?;
    let video_track = renderer.video_stream.clone();
    let video_params = video_track.parameters.as_video().unwrap().clone();

    renderer.resume();

    loop {
        // Blocking read
        let event = read()?;

        match event {
            Event::Key(k) => {
                match k.code {
                    KeyCode::Char('a') => renderer.seek_backwards(Duration::from_secs(5))?,
                    KeyCode::Char('d') => renderer.seek_forward(Duration::from_secs(5))?,
                    KeyCode::Char('r') => renderer.resume(),
                    KeyCode::Char('p') => renderer.pause(),
                    KeyCode::Char('q') => panic!("bye!"),
                    _ => continue,
                };
            }
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if m.row == (video_params.height / 2) && m.column < video_params.width {
                    let pct = m.column as f64 / video_params.width as f64;
                    let time = pct * video_track.duration as f64;
                    renderer.seek(Duration::from_micros(time.round() as u64))?;
                }
            }
            _ => continue,
        };
    }

    // let stdin = io::stdin();
    // for key in stdin.keys() {
    //     let key = key.unwrap();

    //     match key {
    //         Key::Left => renderer.seek_backwards(Duration::from_secs(5))?,
    //         Key::Right => renderer.seek_forward(Duration::from_secs(5))?,
    //         Key::Char('p') => renderer.pause(),
    //         Key::Char('r') => renderer.resume(),
    //         Key::Char('q') => panic!("bye ):"),
    //         _ => continue,
    //     }
    // }

    // renderer.seek(Duration::from_secs(45));
    // renderer.resume();

    renderer.join();
    // let sleeper = SpinSleeper::default();
    // let start = Instant::now();

    // let mut subs: StableVec<(String, Instant)> = StableVec::with_capacity(8);
    // while let Some(slot) = packet_rx.recv_ref() {
    //     if slot.header.data_type == PacketDataType::Subtitle {
    //         let new_subs: Vec<SubRect> = SubRectVec::decode_from(&mut slot.data.as_slice())
    //             .unwrap()
    //             .into_inner();
    //         for sub in new_subs {
    //             subs.push((
    //                 sub.to_string(),
    //                 start + slot.header.timestamp + slot.header.duration,
    //             ));
    //         }
    //         continue;
    //     }

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

    //     stdout.write_all_vectored(&mut slices)?;
    //     stdout.flush()?;
    //     // stdout.write_all_vectored(&mut sub_slice)?;
    //     // stdout.flush()?;
    //     // writeln!(time_log, "prep {:?} render {:?}", prepare_time, write_time);
    //     // time_log.flush()?;
    // }

    Ok(())
}
