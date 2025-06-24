use colorful::palette::{CAM02, DistanceMethod};
use container::SubRect;
use ffmpeg_the_third::codec::subtitle::Subtitle as FFSubFrame;
use ssa::models::events::{EventLine, EventLineParser};
use ssa::models::script_info::ScriptInfo;
use ssa::models::style::*;
use ssa::{LineItemParser, LineStreamParser, SSAParser};
use std::collections::HashMap;
use std::ffi::CStr;

pub trait SubtitleDecoder {
    fn create(data: &str, target_res_x: i64, target_res_y: i64) -> Self
    where
        Self: Sized;
    fn decode_subtitle(&mut self, sub: &FFSubFrame) -> Vec<SubRect>;
}

#[derive(PartialEq, Debug)]
enum AlignX {
    Left,
    Centre,
    Right,
}

#[derive(PartialEq, Debug)]
enum AlignY {
    Bottom,
    Middle,
    Top,
}

#[allow(dead_code)]
pub struct ASSDecoder {
    play_res_x: i64,
    play_res_y: i64,
    target_res_x: i64,
    target_res_y: i64,
    scale_x: f64,
    scale_y: f64,
    parser: LineStreamParser<9, EventLineParser>,
    styles: HashMap<String, StyleInfo>, // l, r, v
}

#[derive(Debug)]
pub struct StyleInfo {
    margin_left: i64,
    margin_right: i64,
    margin_vert: i64,
    align_x: AlignX,
    align_y: AlignY,
    fg: u8,
    bg: u8,
}

impl SubtitleDecoder for ASSDecoder {
    fn create(data: &str, target_res_x: i64, target_res_y: i64) -> ASSDecoder {
        let mut parser = SSAParser::new(data);

        let script_info = parser
            .section()
            .unwrap()
            .as_key_value::<ScriptInfo<'_>>()
            .unwrap();

        let play_res_x = script_info.play_info.play_res_x.unwrap_or(target_res_x);
        let play_res_y = script_info.play_info.play_res_y.unwrap_or(target_res_y);

        let scale_x = (target_res_x as f64) / (play_res_x as f64);
        let scale_y = (target_res_y as f64) / (play_res_y as f64);

        let style_section = loop {
            let section = parser.section().unwrap();
            if StyleParser::validate_section_name(section.title) {
                break section;
            } else {
                section.for_each(|_| ());
                continue;
            }
        };

        let style_parser = style_section
            .as_stream_section::<{ ssa::models::style::MAX_FIELDS }, StyleParser>()
            .unwrap();

        let mut style_map = HashMap::new();

        for style in style_parser {
            let fg = CAM02::closest(&[
                style.primary_color.red,
                style.primary_color.green,
                style.primary_color.blue,
            ]) as u8;
            let bg = CAM02::closest(&[
                style.back_color.red,
                style.back_color.green,
                style.back_color.blue,
            ]) as u8;

            let align_y = match style.alignment {
                1..=3 => AlignY::Bottom,
                4..=6 => AlignY::Middle,
                7..=9 => AlignY::Top,
                _ => panic!(),
            };

            let align_x = match (style.alignment - 1) % 3 {
                0 => AlignX::Left,
                1 => AlignX::Centre,
                2 => AlignX::Right,
                _ => panic!(),
            };

            style_map.insert(
                style.name.to_string(),
                StyleInfo {
                    margin_left: (style.margin_left as f64 * scale_x).round() as i64,
                    margin_right: (style.margin_right as f64 * scale_x).round() as i64,
                    margin_vert: (style.margin_vertical as f64 * scale_y).round() as i64,
                    align_x,
                    align_y,
                    fg,
                    bg,
                },
            );
            // style_map.insert(style.name.to_string(), style.margin_left)
        }

        ASSDecoder {
            play_res_x,
            play_res_y,
            target_res_x,
            target_res_y,
            scale_x,
            scale_y,
            styles: style_map,
            parser: LineStreamParser::new(
                "ReadOrder, Layer, Style, Name, MarginL, MarginR, MarginV, Effect, Text",
            )
            .unwrap(), // mkv hardcode for now
        }
    }

    fn decode_subtitle(&mut self, sub: &FFSubFrame) -> Vec<SubRect> {
        let mut out = Vec::with_capacity(2);
        for rect in sub.rects() {
            let rect_ref = unsafe { rect.as_ptr().as_ref() }.unwrap();

            let line = if !rect_ref.ass.is_null() {
                let Ok(line) = unsafe { CStr::from_ptr(rect_ref.ass) }.to_str() else {
                    continue;
                };
                line
            } else {
                continue;
            };

            let Some(event) = self.parser.parse_line("", line) else {
                continue;
            };

            out.append(&mut self.render_event(&event));
        }

        out
    }
}

impl ASSDecoder {
    fn render_event(&self, event: &EventLine<'_>) -> Vec<SubRect> {
        let style: &StyleInfo = self.styles.get(event.style.as_ref()).unwrap();

        let margin_left = if event.margin_left == 0 {
            style.margin_left
        } else {
            event.margin_left
        };
        let margin_right = if event.margin_right == 0 {
            style.margin_right
        } else {
            event.margin_right
        };
        let margin_vert = if event.margin_vertical == 0 {
            style.margin_vert
        } else {
            event.margin_vertical
        };

        let mut y = match style.align_y {
            AlignY::Bottom => (self.target_res_y / 2) - margin_vert,
            AlignY::Middle => (self.target_res_y / 2) / 2 + 1,
            // AlignY::Top => 1 + self.margin_vert,
            AlignY::Top => margin_vert,
        };

        let max_space = self.target_res_x - (margin_left + margin_right);
        let lines = textwrap::wrap(&event.text, max_space as usize);

        let origin_x = margin_left;

        let mut lines_out = Vec::with_capacity(lines.len());

        for line in lines {
            let line = line.as_ref().trim().replace(r"\N", "");
            let padding_needed = max_space - line.len() as i64;
            let x = match style.align_x {
                AlignX::Right => origin_x + padding_needed,
                AlignX::Centre => origin_x + padding_needed / 2,
                AlignX::Left => origin_x,
            };

            lines_out.push(SubRect {
                fg: style.fg,
                bg: style.bg,
                x: x as i16,
                y: y as i16,
                text: line.to_string(),
            });

            y += 1;
        }

        lines_out
    }
}
