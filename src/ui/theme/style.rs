#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl AnsiColor {
    pub const fn fg_code(self) -> u8 {
        match self {
            Self::Black => 30,
            Self::Red => 31,
            Self::Green => 32,
            Self::Yellow => 33,
            Self::Blue => 34,
            Self::Magenta => 35,
            Self::Cyan => 36,
            Self::White => 37,
            Self::BrightBlack => 90,
            Self::BrightRed => 91,
            Self::BrightGreen => 92,
            Self::BrightYellow => 93,
            Self::BrightBlue => 94,
            Self::BrightMagenta => 95,
            Self::BrightCyan => 96,
            Self::BrightWhite => 97,
        }
    }

    pub const fn bg_code(self) -> u8 {
        match self {
            Self::Black => 40,
            Self::Red => 41,
            Self::Green => 42,
            Self::Yellow => 43,
            Self::Blue => 44,
            Self::Magenta => 45,
            Self::Cyan => 46,
            Self::White => 47,
            Self::BrightBlack => 100,
            Self::BrightRed => 101,
            Self::BrightGreen => 102,
            Self::BrightYellow => 103,
            Self::BrightBlue => 104,
            Self::BrightMagenta => 105,
            Self::BrightCyan => 106,
            Self::BrightWhite => 107,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor(pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Ansi(AnsiColor),
    Rgb(RgbColor),
}

impl From<AnsiColor> for Color {
    fn from(c: AnsiColor) -> Self {
        Self::Ansi(c)
    }
}

impl From<RgbColor> for Color {
    fn from(rgb: RgbColor) -> Self {
        Self::Rgb(rgb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub dimmed: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
}

impl Style {
    pub const fn new() -> Self {
        Self {
            bold: false,
            dimmed: false,
            italic: false,
            strikethrough: false,
            fg: None,
            bg: None,
        }
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub const fn dimmed(mut self) -> Self {
        self.dimmed = true;
        self
    }

    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub const fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    pub const fn fg_color(mut self, fg: Option<Color>) -> Self {
        self.fg = fg;
        self
    }

    pub const fn bg_color(mut self, bg: Option<Color>) -> Self {
        self.bg = bg;
        self
    }

    pub const fn get_fg_color(&self) -> Option<Color> {
        self.fg
    }

    pub const fn get_bg_color(&self) -> Option<Color> {
        self.bg
    }

    pub fn render(&self) -> AnsiDisplay {
        AnsiDisplay { style: *self }
    }

    pub fn render_reset(&self) -> AnsiReset {
        AnsiReset
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AnsiDisplay {
    style: Style,
}

impl std::fmt::Display for AnsiDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.style.bold {
            write!(f, "\x1b[1m")?;
        }
        if self.style.dimmed {
            write!(f, "\x1b[2m")?;
        }
        if self.style.italic {
            write!(f, "\x1b[3m")?;
        }
        if self.style.strikethrough {
            write!(f, "\x1b[9m")?;
        }
        if let Some(fg) = self.style.fg {
            match fg {
                Color::Ansi(ansi) => write!(f, "\x1b[{}m", ansi.fg_code())?,
                Color::Rgb(rgb) => write!(f, "\x1b[38;2;{};{};{}m", rgb.0, rgb.1, rgb.2)?,
            }
        }
        if let Some(bg) = self.style.bg {
            match bg {
                Color::Ansi(ansi) => write!(f, "\x1b[{}m", ansi.bg_code())?,
                Color::Rgb(rgb) => write!(f, "\x1b[48;2;{};{};{}m", rgb.0, rgb.1, rgb.2)?,
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AnsiReset;

impl std::fmt::Display for AnsiReset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\x1b[0m")
    }
}

impl std::fmt::Display for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "\x1b[0m")
        } else {
            write!(f, "{}", self.render())
        }
    }
}

impl From<AnsiColor> for ratatui::style::Color {
    fn from(c: AnsiColor) -> Self {
        match c {
            AnsiColor::Black => Self::Black,
            AnsiColor::Red => Self::Red,
            AnsiColor::Green => Self::Green,
            AnsiColor::Yellow => Self::Yellow,
            AnsiColor::Blue => Self::Blue,
            AnsiColor::Magenta => Self::Magenta,
            AnsiColor::Cyan => Self::Cyan,
            AnsiColor::White => Self::White,
            AnsiColor::BrightBlack => Self::DarkGray,
            AnsiColor::BrightRed => Self::LightRed,
            AnsiColor::BrightGreen => Self::LightGreen,
            AnsiColor::BrightYellow => Self::LightYellow,
            AnsiColor::BrightBlue => Self::LightBlue,
            AnsiColor::BrightMagenta => Self::LightMagenta,
            AnsiColor::BrightCyan => Self::LightCyan,
            AnsiColor::BrightWhite => Self::Gray,
        }
    }
}

impl From<RgbColor> for ratatui::style::Color {
    fn from(rgb: RgbColor) -> Self {
        Self::Rgb(rgb.0, rgb.1, rgb.2)
    }
}

impl From<Color> for ratatui::style::Color {
    fn from(c: Color) -> Self {
        match c {
            Color::Ansi(ansi) => ansi.into(),
            Color::Rgb(rgb) => rgb.into(),
        }
    }
}

impl From<Style> for ratatui::style::Style {
    fn from(s: Style) -> Self {
        let mut style = Self::default();
        if s.bold {
            style = style.add_modifier(ratatui::style::Modifier::BOLD);
        }
        if s.dimmed {
            style = style.add_modifier(ratatui::style::Modifier::DIM);
        }
        if s.italic {
            style = style.add_modifier(ratatui::style::Modifier::ITALIC);
        }
        if s.strikethrough {
            style = style.add_modifier(ratatui::style::Modifier::CROSSED_OUT);
        }
        if let Some(fg) = s.fg {
            style = style.fg(fg.into());
        }
        if let Some(bg) = s.bg {
            style = style.bg(bg.into());
        }
        style
    }
}
