use std::{collections::HashMap, env, path::Path};

use const_gen::{const_array_declaration, const_declaration, CompileConst, CompileConstArray};
use kasi_kule::{Jab, UCS};
use lab::Lab;
use prettypretty::{
    termco::AnsiColor,
    theme::{Theme, VGA_COLORS},
};
use prettytty::{opt::Options, Connection};

// from https://docs.rs/prettypretty/latest/src/prettypretty/termco.rs.html#506
fn xterm216_to_rgb(idx: u8) -> [u8; 3] {
    let mut b = idx - 16;
    let r = b / 36;
    b -= r * 36;
    let g = b / 6;
    b -= g * 6;

    // https://docs.rs/prettypretty/latest/src/prettypretty/termco.rs.html#547
    fn convert(value: u8) -> u8 {
        if value == 0 {
            0
        } else {
            55 + 40 * value
        }
    }

    [convert(r), convert(g), convert(b)]
}

fn gray_to_rgb(idx: u8) -> [u8; 3] {
    let idx = idx - 232;
    let level = 8 + 10 * idx;
    [level, level, level]
}

fn main() {
    let theme = match Connection::with_options(Options::with_log()) {
        Ok(tty) => Theme::query(&tty).unwrap_or(VGA_COLORS),
        Err(_) => VGA_COLORS,
    };

    let mut palette = Vec::with_capacity(256);

    for idx in 0..=u8::MAX {
        match idx {
            0..=15 => {
                palette.push(theme[AnsiColor::try_from(idx).unwrap()].to_24bit());
            }
            16..=231 => palette.push(xterm216_to_rgb(idx)),
            232.. => palette.push(gray_to_rgb(idx)),
        }
    }

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("const_gen.rs");

    let reverse_palette: HashMap<[u8; 3], u8> =
        HashMap::from_iter(palette.iter().enumerate().map(|(idx, v)| (*v, idx as u8)));

    let lab_palette = Vec::from_iter(palette.iter().map(|c| {
        let lab = Lab::from_rgb(c);
        (lab.l, lab.a, lab.b)
    }));

    let lab_palette_flattened =
        Vec::from_iter(lab_palette.iter().flat_map(|(l, a, b)| [*l, *a, *b, 0.0]));

    let jab_palette = Vec::from_iter(palette.iter().map(|c| {
        let jab = Jab::<UCS>::from(*c);
        (jab.J, jab.a, jab.b)
    }));

    let jab_palette_flattened =
        Vec::from_iter(jab_palette.iter().flat_map(|(j, a, b)| [*j, *a, *b, 0.0]));

    let palette_fg_codes = Vec::from_iter((0..=255u8).map(|v| format!("\x1B[38;5;{v}m")));
    let palette_bg_codes = Vec::from_iter((0..=255u8).map(|v| format!("\x1B[48;5;{v}m")));

    let reverse_palette_fg_codes: HashMap<[u8; 3], String> = HashMap::from_iter(
        palette
            .iter()
            .enumerate()
            .map(|(idx, color)| (*color, format!("\x1B[38;5;{idx}m"))),
    );

    let reverse_palette_bg_codes: HashMap<[u8; 3], String> = HashMap::from_iter(
        palette
            .iter()
            .enumerate()
            .map(|(idx, color)| (*color, format!("\x1B[48;5;{idx}m"))),
    );

    let decls = vec![
        const_array_declaration!(pub PALETTE = palette),
        const_array_declaration!(pub LAB_PALETTE = lab_palette),
        const_array_declaration!(pub LAB_PALETTE_FLATTENED = lab_palette_flattened),
        const_array_declaration!(pub JAB_PALETTE = jab_palette),
        const_array_declaration!(pub JAB_PALETTE_FLATTENED = jab_palette_flattened),
        const_array_declaration!(pub PALETTE_FG_CODES = palette_fg_codes),
        const_array_declaration!(pub PALETTE_BG_CODES = palette_bg_codes),
        const_declaration!(pub REVERSE_PALETTE = reverse_palette),
        const_declaration!(pub REVERSE_PALETTE_FG_CODES = reverse_palette_fg_codes),
        const_declaration!(pub REVERSE_PALETTE_BG_CODES = reverse_palette_bg_codes),
    ]
    .join("\n");
    std::fs::write(&dest_path, decls).unwrap();
}
