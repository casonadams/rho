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
            ("indent_size" | "tab_width", true) => size = value.parse().ok().or(size),
            _ => {}
        }
    }
    indent.tab_size = size.unwrap_or(indent.tab_size);
}

/// The indentation for `file`, from the `.editorconfig` files between it and `workspace`.
pub fn indent_for(workspace: &Path, file: &Path) -> Indent {
    let file_name = file.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let mut configs = Vec::new();
    for dir in file.ancestors().skip(1).take_while(|dir| dir.starts_with(workspace)) {
        let Ok(text) = std::fs::read_to_string(dir.join(".editorconfig")) else {
            continue;
        };
        let root = text
            .lines()
            .any(|line| line.replace(' ', "").eq_ignore_ascii_case("root=true"));
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

    #[test]
    fn nearer_files_and_later_sections_override_and_unmatched_sections_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("src");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(
            dir.path().join(".editorconfig"),
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

        assert_eq!(indent_for(dir.path(), &sub.join("lib.rs")), spaces);
        assert_eq!(indent_for(dir.path(), &sub.join("lib.go")), tab);
        assert_eq!(indent_for(dir.path(), &dir.path().join("x.py")).tab_size, 2);
    }

    #[test]
    fn without_a_config_the_default_is_four_spaces() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(indent_for(dir.path(), &dir.path().join("a.rs")), Indent::default());
    }
}
