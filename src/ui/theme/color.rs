use anstyle::{AnsiColor, Color, RgbColor};

pub fn parse_color(input: &str) -> Option<Color> {
    let trimmed = input.trim();
    if let Some(hex) = trimmed.strip_prefix('#') {
        parse_hex_color(hex)
    } else {
        parse_named_ansi_color(trimmed)
    }
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(RgbColor(r, g, b)))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some(Color::Rgb(RgbColor(r * 17, g * 17, b * 17)))
        }
        _ => None,
    }
}

fn parse_named_ansi_color(name: &str) -> Option<Color> {
    let normalized = name.to_ascii_lowercase().replace('-', "_");
    let ansi = match normalized.as_str() {
        "black" => AnsiColor::Black,
        "red" => AnsiColor::Red,
        "green" => AnsiColor::Green,
        "yellow" => AnsiColor::Yellow,
        "blue" => AnsiColor::Blue,
        "magenta" => AnsiColor::Magenta,
        "cyan" => AnsiColor::Cyan,
        "white" => AnsiColor::White,
        "bright_black" | "gray" | "grey" => AnsiColor::BrightBlack,
        "bright_red" => AnsiColor::BrightRed,
        "bright_green" => AnsiColor::BrightGreen,
        "bright_yellow" => AnsiColor::BrightYellow,
        "bright_blue" => AnsiColor::BrightBlue,
        "bright_magenta" => AnsiColor::BrightMagenta,
        "bright_cyan" => AnsiColor::BrightCyan,
        "bright_white" => AnsiColor::BrightWhite,
        _ => return None,
    };
    Some(Color::Ansi(ansi))
}
