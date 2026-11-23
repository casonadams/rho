//! The indentation a file's `.editorconfig` asks for: the sections `*`, `*.ext`, `*.{a,b}` and an
//! exact file name, from the workspace root down to the file's directory (nearer files win).

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    pub tab_size: u32,
    pub insert_spaces: bool,
}

impl Default for Indent {
    fn default() -> Self {
        Self {
            tab_size: 4,
            insert_spaces: true,
        }
    }
}

fn section_matches(pattern: &str, file_name: &str) -> bool {
    let extension = file_name.rsplit_once('.').map(|(_, ext)| ext);
    match pattern {
        "*" => true,
        _ => match pattern.strip_prefix("*.") {
            Some(rest) => match rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                Some(list) => list.split(',').any(|ext| Some(ext.trim()) == extension),
                None => Some(rest) == extension,
            },
            None => pattern == file_name,
        },
    }
}

fn apply(config: &str, file_name: &str, indent: &mut Indent) {
    let mut in_section = false;
    let mut size: Option<u32> = None;
    let mut width: Option<u32> = None;
    let positive = |value: &str| value.parse::<u32>().ok().filter(|n| *n > 0);
    for line in config.lines().map(str::trim) {
        if let Some(pattern) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_section = section_matches(pattern, file_name);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim().to_ascii_lowercase());
        match (key.as_str(), in_section) {
            ("indent_style", true) => indent.insert_spaces = value != "tab",
            ("indent_size", true) => size = positive(&value).or(size),
            ("tab_width", true) => width = positive(&value).or(width),
            _ => {}
        }
    }
    indent.tab_size = size.or(width).unwrap_or(indent.tab_size);
}

/// `root = true` before the first section header.
fn is_root(config: &str) -> bool {
    config
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .any(|line| line.replace(' ', "").eq_ignore_ascii_case("root=true"))
}

/// The indentation for `file`, from the `.editorconfig` files between it and `workspace`.
pub fn indent_for(workspace: &Path, file: &Path) -> Indent {
    let file_name = file.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let mut configs = Vec::new();
    for dir in file.ancestors().skip(1).take_while(|dir| dir.starts_with(workspace)) {
        let Ok(text) = std::fs::read_to_string(dir.join(".editorconfig")) else {
            continue;
        };
        let root = is_root(&text);
        configs.push(text);
        if root {
            break;
        }
    }
    let mut indent = Indent::default();
    for config in configs.iter().rev() {
        apply(config, file_name, &mut indent);
    }
    indent
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_test_support::TestDir;

    #[test]
    fn nearer_files_and_later_sections_override_and_unmatched_sections_are_ignored() {
        let dir = TestDir::new();
        let sub = dir.as_ref().join("src");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(
            dir.as_ref().join(".editorconfig"),
            "root = true\n[*]\nindent_style = tab\nindent_size = 8\n[*.py]\nindent_size = 2\n",
        )
        .unwrap();
        std::fs::write(
            sub.join(".editorconfig"),
            "[*.{rs,toml}]\nindent_style = space\nindent_size = 3\n",
        )
        .unwrap();
        let tab = Indent {
            tab_size: 8,
            insert_spaces: false,
        };
        let spaces = Indent {
            tab_size: 3,
            insert_spaces: true,
        };

        assert_eq!(indent_for(dir.as_ref(), &sub.join("lib.rs")), spaces);
        assert_eq!(indent_for(dir.as_ref(), &sub.join("lib.go")), tab);
        assert_eq!(indent_for(dir.as_ref(), &dir.as_ref().join("x.py")).tab_size, 2);
    }

    fn indent_of(configs: &[(&str, &str)], file: &str) -> Indent {
        let dir = TestDir::new();
        for (path, text) in configs {
            let path = dir.as_ref().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        indent_for(dir.as_ref(), &dir.as_ref().join(file))
    }

    #[test]
    fn indent_size_wins_over_tab_width_in_either_order() {
        for config in [
            "[*]\ntab_width = 8\nindent_size = 2\n",
            "[*]\nindent_size = 2\ntab_width = 8\n",
        ] {
            assert_eq!(indent_of(&[(".editorconfig", config)], "a.rs").tab_size, 2, "{config}");
        }
        assert_eq!(
            indent_of(&[(".editorconfig", "[*]\ntab_width = 8\n")], "a.rs").tab_size,
            8
        );
    }

    #[test]
    fn root_true_counts_only_in_the_preamble() {
        let parent = (".editorconfig", "[*]\nindent_size = 8\n");
        let not_root = ("sub/.editorconfig", "[*.py]\nroot = true\n");
        assert_eq!(indent_of(&[parent, not_root], "sub/x.rs").tab_size, 8);
        let root = ("sub/.editorconfig", "root = true\n[*.py]\nindent_size = 2\n");
        assert_eq!(indent_of(&[parent, root], "sub/x.rs").tab_size, 4);
    }

    #[test]
    fn a_zero_indent_size_falls_back_to_four() {
        assert_eq!(
            indent_of(&[(".editorconfig", "[*]\nindent_size = 0\n")], "a.rs").tab_size,
            4
        );
    }

    #[test]
    fn without_a_config_the_default_is_four_spaces() {
        let dir = TestDir::new();
        assert_eq!(indent_for(dir.as_ref(), &dir.as_ref().join("a.rs")), Indent::default());
    }
}
