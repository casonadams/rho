//! Language families and path classes used to decide which review rules apply.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    Js,
    Python,
    Go,
    Java,
    C,
    Shell,
    Other,
}

pub const CODE: &[Lang] = &[
    Lang::Rust,
    Lang::Js,
    Lang::Python,
    Lang::Go,
    Lang::Java,
    Lang::C,
    Lang::Shell,
];

impl Lang {
    #[must_use]
    pub fn from_path(path: &str) -> Self {
        let ext = path.rsplit_once('.').map_or("", |(_, ext)| ext);
        match ext {
            "rs" => Self::Rust,
            "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" => Self::Js,
            "py" => Self::Python,
            "go" => Self::Go,
            "java" | "kt" | "kts" => Self::Java,
            "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" => Self::C,
            "sh" | "bash" | "zsh" => Self::Shell,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub fn is_code(self) -> bool {
        self != Self::Other
    }

    /// The marker that starts a comment running to the end of the line.
    #[must_use]
    pub fn line_comment(self) -> Option<&'static str> {
        match self {
            Self::Rust | Self::Js | Self::Go | Self::Java | Self::C => Some("//"),
            Self::Python | Self::Shell => Some("#"),
            Self::Other => None,
        }
    }

    /// Spaces per indentation level, used to measure nesting.
    #[must_use]
    pub fn indent_unit(self) -> usize {
        if self == Self::Js { 2 } else { 4 }
    }

    #[must_use]
    pub fn is_comment(self, trimmed: &str) -> bool {
        let Some(marker) = self.line_comment() else {
            return false;
        };
        let line_comment = trimmed.starts_with(marker) && !(self == Self::Shell && trimmed.starts_with("#!"));
        let block = self != Self::Python
            && self != Self::Shell
            && (trimmed.starts_with("/*") || trimmed.starts_with("* ") || trimmed == "*" || trimmed.starts_with("*/"));
        line_comment || block
    }
}

/// Paths that are generated, vendored, or lock files: nothing in them is reviewed.
#[must_use]
pub fn is_ignored_path(path: &str) -> bool {
    const DIRS: [&str; 7] = [
        "target",
        "node_modules",
        "vendor",
        "dist",
        ".git",
        "generated",
        "__pycache__",
    ];
    const SUFFIXES: [&str; 7] = [
        ".lock",
        ".min.js",
        ".min.css",
        ".snap",
        ".map",
        "package-lock.json",
        ".svg",
    ];
    path.split('/').any(|part| DIRS.contains(&part)) || SUFFIXES.iter().any(|s| path.ends_with(s))
}

#[must_use]
pub fn is_test_path(path: &str) -> bool {
    const WORDS: [&str; 5] = ["test", "tests", "spec", "specs", "testdata"];
    path.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| WORDS.contains(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_come_from_extensions() {
        let cases = [
            ("a/b.rs", Lang::Rust),
            ("x.tsx", Lang::Js),
            ("x.py", Lang::Python),
            ("x.go", Lang::Go),
            ("X.kt", Lang::Java),
            ("x.hpp", Lang::C),
            ("x.sh", Lang::Shell),
            ("README.md", Lang::Other),
            ("Makefile", Lang::Other),
        ];
        for (path, lang) in cases {
            assert_eq!(Lang::from_path(path), lang, "{path}");
        }
    }

    #[test]
    fn test_paths_are_recognized_by_segment_not_substring() {
        let tests = [
            "tests/a.rs",
            "src/a_test.go",
            "src/test_a.py",
            "web/a.spec.ts",
            "a/__tests__/b.js",
            "src/ui/transcript_tests.rs",
            "crates/rho-test-support/src/lib.rs",
        ];
        let code = ["src/latest.rs", "src/contest/a.rs", "src/attestation.py", "src/main.rs"];
        assert!(tests.iter().all(|p| is_test_path(p)));
        assert!(code.iter().all(|p| !is_test_path(p)));
    }

    #[test]
    fn generated_and_vendored_paths_are_ignored() {
        assert!(is_ignored_path("Cargo.lock"));
        assert!(is_ignored_path("web/node_modules/x/index.js"));
        assert!(is_ignored_path("target/debug/a.rs"));
        assert!(!is_ignored_path("src/build.rs"));
    }

    #[test]
    fn comments_depend_on_the_language() {
        assert!(Lang::Rust.is_comment("// x"));
        assert!(Lang::Rust.is_comment("/// doc"));
        assert!(Lang::Js.is_comment("* line"));
        assert!(Lang::Python.is_comment("# x"));
        assert!(!Lang::Rust.is_comment("#[allow(x)]"));
        assert!(!Lang::Shell.is_comment("#!/bin/sh"));
        assert!(!Lang::Other.is_comment("// x"));
    }
}
