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
    use test_case::test_case;

    #[test_case("a/b.rs", Lang::Rust; "rust")]
    #[test_case("x.tsx", Lang::Js; "tsx")]
    #[test_case("x.py", Lang::Python; "python")]
    #[test_case("x.go", Lang::Go; "go")]
    #[test_case("X.kt", Lang::Java; "kotlin")]
    #[test_case("x.hpp", Lang::C; "cpp header")]
    #[test_case("x.sh", Lang::Shell; "shell")]
    #[test_case("README.md", Lang::Other; "markdown")]
    #[test_case("Makefile", Lang::Other; "no extension")]
    fn languages_come_from_extensions(path: &str, lang: Lang) {
        assert_eq!(Lang::from_path(path), lang);
    }

    #[test_case("tests/a.rs"; "tests dir")]
    #[test_case("src/a_test.go"; "test suffix")]
    #[test_case("src/test_a.py"; "test prefix")]
    #[test_case("web/a.spec.ts"; "spec segment")]
    #[test_case("a/__tests__/b.js"; "dunder tests dir")]
    #[test_case("src/ui/transcript_tests.rs"; "tests suffix")]
    #[test_case("crates/rho-test-support/src/lib.rs"; "hyphenated test segment")]
    fn test_paths_are_recognized_by_segment(path: &str) {
        assert!(is_test_path(path));
    }

    #[test_case("src/latest.rs"; "test inside word")]
    #[test_case("src/contest/a.rs"; "test inside dir name")]
    #[test_case("src/attestation.py"; "test inside longer word")]
    #[test_case("src/main.rs"; "plain source")]
    fn code_paths_containing_test_as_a_substring_are_not_test_paths(path: &str) {
        assert!(!is_test_path(path));
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
