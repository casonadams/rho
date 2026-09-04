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
        self.def.into_theme(self.name)
    }
}

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
            def: ThemeDef {
                prompt: Some("#89b4fa".into()),
                highlight: Some("#89dceb".into()),
                tool_header: Some("#cba6f7".into()),
                tool_ok: Some("#a6e3a1".into()),
                tool_err: Some("#f38ba8".into()),
                code_inline: Some("#f9e2af".into()),
                heading_h1: Some("#cba6f7".into()),
                heading_h2: Some("#89b4fa".into()),
                heading_h3: Some("#94e2d5".into()),
                dimmed: Some("#6c7086".into()),
                user_message_bg: Some("#181825".into()),
                tool_success_bg: Some("#181825".into()),
                tool_error_bg: Some("#311b24".into()),
                ..Default::default()
            },
        },
        BuiltinTheme {
            name: "nord",
            description: "Arctic, north-bluish clean palette",
            is_light: false,
            def: ThemeDef {
                prompt: Some("#88c0d0".into()),
                highlight: Some("#81a1c1".into()),
                tool_header: Some("#81a1c1".into()),
                tool_ok: Some("#a3be8c".into()),
                tool_err: Some("#bf616a".into()),
                code_inline: Some("#ebcb8b".into()),
                heading_h1: Some("#88c0d0".into()),
                heading_h2: Some("#81a1c1".into()),
                heading_h3: Some("#5e81ac".into()),
                dimmed: Some("#4c566a".into()),
                user_message_bg: Some("#2e3440".into()),
                tool_success_bg: Some("#2e3440".into()),
                tool_error_bg: Some("#3b2d35".into()),
                ..Default::default()
            },
        },
        BuiltinTheme {
            name: "tokyo-night",
            description: "Modern dark neon purple and blue",
            is_light: false,
            def: ThemeDef {
                prompt: Some("#7aa2f7".into()),
                highlight: Some("#2ac3de".into()),
                tool_header: Some("#bb9af7".into()),
                tool_ok: Some("#9ece6a".into()),
                tool_err: Some("#f7768e".into()),
                code_inline: Some("#e0af68".into()),
                heading_h1: Some("#bb9af7".into()),
                heading_h2: Some("#7aa2f7".into()),
                heading_h3: Some("#7dcfff".into()),
                dimmed: Some("#565f89".into()),
                user_message_bg: Some("#16161e".into()),
                tool_success_bg: Some("#16161e".into()),
                tool_error_bg: Some("#2d1f2d".into()),
                ..Default::default()
            },
        },
        BuiltinTheme {
            name: "dracula",
            description: "Vibrant dark purple and pink",
            is_light: false,
            def: ThemeDef {
                prompt: Some("#bd93f9".into()),
                highlight: Some("#ff79c6".into()),
                tool_header: Some("#bd93f9".into()),
                tool_ok: Some("#50fa7b".into()),
                tool_err: Some("#ff5555".into()),
                code_inline: Some("#f1fa8c".into()),
                heading_h1: Some("#bd93f9".into()),
                heading_h2: Some("#ff79c6".into()),
                heading_h3: Some("#8be9fd".into()),
                dimmed: Some("#6272a4".into()),
                user_message_bg: Some("#21222c".into()),
                tool_success_bg: Some("#21222c".into()),
                tool_error_bg: Some("#351f28".into()),
                ..Default::default()
            },
        },
        BuiltinTheme {
            name: "gruvbox",
            description: "Retro groove warm dark palette",
            is_light: false,
            def: ThemeDef {
                prompt: Some("#fe8019".into()),
                highlight: Some("#83a598".into()),
                tool_header: Some("#d3869b".into()),
                tool_ok: Some("#b8bb26".into()),
                tool_err: Some("#fb4934".into()),
                code_inline: Some("#fabd2f".into()),
                heading_h1: Some("#fe8019".into()),
                heading_h2: Some("#83a598".into()),
                heading_h3: Some("#8ec07c".into()),
                dimmed: Some("#928374".into()),
                user_message_bg: Some("#282828".into()),
                tool_success_bg: Some("#282828".into()),
                tool_error_bg: Some("#382323".into()),
                ..Default::default()
            },
        },
        BuiltinTheme {
            name: "monokai",
            description: "High-contrast classic code palette",
            is_light: false,
            def: ThemeDef {
                prompt: Some("#66d9ef".into()),
                highlight: Some("#a6e22e".into()),
                tool_header: Some("#ae81ff".into()),
                tool_ok: Some("#a6e22e".into()),
                tool_err: Some("#f92672".into()),
                code_inline: Some("#fd971f".into()),
                heading_h1: Some("#66d9ef".into()),
                heading_h2: Some("#ae81ff".into()),
                heading_h3: Some("#a6e22e".into()),
                dimmed: Some("#75715e".into()),
                user_message_bg: Some("#272822".into()),
                tool_success_bg: Some("#272822".into()),
                tool_error_bg: Some("#3a2028".into()),
                ..Default::default()
            },
        },
        BuiltinTheme {
            name: "one-dark",
            description: "Balanced modern dark palette",
            is_light: false,
            def: ThemeDef {
                prompt: Some("#61afef".into()),
                highlight: Some("#56b6c2".into()),
                tool_header: Some("#c678dd".into()),
                tool_ok: Some("#98c379".into()),
                tool_err: Some("#e06c75".into()),
                code_inline: Some("#e5c07b".into()),
                heading_h1: Some("#c678dd".into()),
                heading_h2: Some("#61afef".into()),
                heading_h3: Some("#56b6c2".into()),
                dimmed: Some("#5c6370".into()),
                user_message_bg: Some("#21252b".into()),
                tool_success_bg: Some("#21252b".into()),
                tool_error_bg: Some("#342329".into()),
                ..Default::default()
            },
        },
        BuiltinTheme {
            name: "solarized-dark",
            description: "Precision cyan and blue low-contrast dark",
            is_light: false,
            def: ThemeDef {
                prompt: Some("#2aa198".into()),
                highlight: Some("#268bd2".into()),
                tool_header: Some("#6c71c4".into()),
                tool_ok: Some("#859900".into()),
                tool_err: Some("#dc322f".into()),
                code_inline: Some("#b58900".into()),
                heading_h1: Some("#2aa198".into()),
                heading_h2: Some("#268bd2".into()),
                heading_h3: Some("#6c71c4".into()),
                dimmed: Some("#586e75".into()),
                user_message_bg: Some("#073642".into()),
                tool_success_bg: Some("#073642".into()),
                tool_error_bg: Some("#382020".into()),
                ..Default::default()
            },
        },
        BuiltinTheme {
            name: "catppuccin-latte",
            description: "Soothing warm light palette for light terminals",
            is_light: true,
            def: ThemeDef {
                prompt: Some("#1e66f5".into()),
                highlight: Some("#04a5e5".into()),
                tool_header: Some("#8839ef".into()),
                tool_ok: Some("#40a02b".into()),
                tool_err: Some("#d20f39".into()),
                code_inline: Some("#df8e1d".into()),
                heading_h1: Some("#8839ef".into()),
                heading_h2: Some("#1e66f5".into()),
                heading_h3: Some("#179299".into()),
                dimmed: Some("#9ca0b0".into()),
                user_message_bg: Some("#e6e9ef".into()),
                tool_success_bg: Some("#e6e9ef".into()),
                tool_error_bg: Some("#fcdada".into()),
                ..Default::default()
            },
        },
    ]
}
