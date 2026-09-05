use super::Theme;
use super::definition::ThemeDef;

pub struct BuiltinTheme {
    pub name: &'static str,
    pub description: &'static str,
    pub is_light: bool,
    pub def: ThemeDef,
}

impl BuiltinTheme {
    pub fn to_theme(&self) -> Theme {
        let mut def = self.def.clone();
        def.is_light = self.is_light;
        def.into_theme(self.name)
    }
}

fn def(palette: (&str, &str, [&str; 8], [&str; 8])) -> ThemeDef {
    let (background, foreground, colors, bright_colors) = palette;
    ThemeDef {
        background: Some(background.into()),
        foreground: Some(foreground.into()),
        black: Some(colors[0].into()),
        red: Some(colors[1].into()),
        green: Some(colors[2].into()),
        yellow: Some(colors[3].into()),
        blue: Some(colors[4].into()),
        magenta: Some(colors[5].into()),
        cyan: Some(colors[6].into()),
        white: Some(colors[7].into()),
        bright_black: Some(bright_colors[0].into()),
        bright_red: Some(bright_colors[1].into()),
        bright_green: Some(bright_colors[2].into()),
        bright_yellow: Some(bright_colors[3].into()),
        bright_blue: Some(bright_colors[4].into()),
        bright_magenta: Some(bright_colors[5].into()),
        bright_cyan: Some(bright_colors[6].into()),
        bright_white: Some(bright_colors[7].into()),
        ..Default::default()
    }
}

// Palettes sourced from walh-shell's theme definitions: background, foreground,
// and color01-06 are taken verbatim; color00, color07, color08, color15, and the
// bright slots follow walh-shell's derivation rules for the missing entries.
pub fn builtin_themes() -> Vec<BuiltinTheme> {
    vec![
        BuiltinTheme {
            name: "default",
            description: "Standard 16-color terminal ANSI palette",
            is_light: false,
            def: ThemeDef::default(),
        },
        BuiltinTheme {
            name: "catppuccin",
            description: "Soothing dark pastel palette (Mocha)",
            is_light: false,
            def: def((
                "#181825",
                "#cdd6f4",
                [
                    "#2f2f3a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#cba6f7", "#94e2d5", "#d2daf5",
                ],
                [
                    "#72778c", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#cba6f7", "#94e2d5", "#f5f6fc",
                ],
            )),
        },
        BuiltinTheme {
            name: "nord",
            description: "Arctic, north-bluish clean palette",
            is_light: false,
            def: def((
                "#2e3440",
                "#d8dee9",
                [
                    "#424853", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#dbe1eb",
                ],
                [
                    "#838994", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#f7f8fa",
                ],
            )),
        },
        BuiltinTheme {
            name: "tokyo-night",
            description: "Modern dark neon purple and blue",
            is_light: false,
            def: def((
                "#1a1b26",
                "#a9b1d6",
                [
                    "#30313b", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#ad8ee6", "#449dab", "#b1b8da",
                ],
                [
                    "#61667e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#ad8ee6", "#449dab", "#edeff6",
                ],
            )),
        },
        BuiltinTheme {
            name: "dracula",
            description: "Vibrant dark purple and pink",
            is_light: false,
            def: def((
                "#282936",
                "#e9e9f4",
                [
                    "#3d3e4a", "#ea51b2", "#ebff87", "#00f769", "#62d6e8", "#b45bcf", "#a1efe4", "#ebebf5",
                ],
                [
                    "#888995", "#ea51b2", "#ebff87", "#00f769", "#62d6e8", "#b45bcf", "#a1efe4", "#fafafc",
                ],
            )),
        },
        BuiltinTheme {
            name: "gruvbox",
            description: "Retro groove warm dark palette",
            is_light: false,
            def: def((
                "#282828",
                "#d5c4a1",
                [
                    "#3d3d3d", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#d9c9aa",
                ],
                [
                    "#7e7664", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#f6f3ec",
                ],
            )),
        },
        BuiltinTheme {
            name: "monokai",
            description: "High-contrast classic code palette",
            is_light: false,
            def: def((
                "#272822",
                "#f8f8f2",
                [
                    "#3c3d38", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4", "#f8f8f3",
                ],
                [
                    "#8f908a", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4", "#fdfdfc",
                ],
            )),
        },
        BuiltinTheme {
            name: "one-dark",
            description: "Balanced modern dark palette",
            is_light: false,
            def: def((
                "#282c34",
                "#abb2bf",
                [
                    "#3d4148", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#b3b9c5",
                ],
                [
                    "#696f79", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#eeeff2",
                ],
            )),
        },
        BuiltinTheme {
            name: "solarized-dark",
            description: "Precision cyan and blue low-contrast dark",
            is_light: false,
            def: def((
                "#002b36",
                "#93a1a1",
                [
                    "#19404a", "#dc322f", "#859900", "#b58900", "#268bd2", "#6c71c4", "#2aa198", "#9daaaa",
                ],
                [
                    "#49666b", "#dc322f", "#859900", "#b58900", "#268bd2", "#6c71c4", "#2aa198", "#e9ecec",
                ],
            )),
        },
        BuiltinTheme {
            name: "catppuccin-latte",
            description: "Soothing warm light palette for light terminals",
            is_light: true,
            def: def((
                "#eff1f5",
                "#4c4f69",
                [
                    "#d7d8dc", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#04a5e5", "#5d6078",
                ],
                [
                    "#9da0af", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#04a5e5", "#3c3f54",
                ],
            )),
        },
    ]
}
