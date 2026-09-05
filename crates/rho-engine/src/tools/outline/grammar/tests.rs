use super::*;
use std::path::Path;

#[test]
fn test_detect_supported_languages() {
    let cases = [
        ("src/main.rs", Some(SupportedLanguage::Rust)),
        ("index.ts", Some(SupportedLanguage::TypeScript)),
        ("components/App.tsx", Some(SupportedLanguage::Tsx)),
        ("server.js", Some(SupportedLanguage::JavaScript)),
        ("client.jsx", Some(SupportedLanguage::JavaScript)),
        ("module.mjs", Some(SupportedLanguage::JavaScript)),
        ("common.cjs", Some(SupportedLanguage::JavaScript)),
        ("script.py", Some(SupportedLanguage::Python)),
        ("types.pyi", Some(SupportedLanguage::Python)),
        ("main.go", Some(SupportedLanguage::Go)),
    ];

    for (path_str, expected) in cases {
        let path = Path::new(path_str);
        let detected = SupportedLanguage::from_path(path);
        assert_eq!(detected, expected, "Failed for path: {path_str}");
        assert!(detect_language(path).is_some());
    }
}

#[test]
fn test_unsupported_extensions_return_none() {
    let unsupported = [
        "README.md",
        "data.json",
        "Cargo.toml",
        "notes.txt",
        "style.css",
        "Makefile",
        ".gitignore",
        "",
    ];

    for path_str in unsupported {
        let path = Path::new(path_str);
        assert_eq!(SupportedLanguage::from_path(path), None);
        assert!(detect_language(path).is_none());
    }
}

#[test]
fn test_parser_creation_and_snippet_parsing() {
    let snippets = [
        (SupportedLanguage::Rust, "pub fn add(a: i32, b: i32) -> i32 { a + b }"),
        (
            SupportedLanguage::TypeScript,
            "export function greet(name: string): string { return name; }",
        ),
        (
            SupportedLanguage::Tsx,
            "export const Component = () => <div>Hello</div>;",
        ),
        (
            SupportedLanguage::JavaScript,
            "function compute(x, y) { return x * y; }",
        ),
        (
            SupportedLanguage::Python,
            "def calculate(value: int) -> int:\n    return value * 2",
        ),
        (
            SupportedLanguage::Go,
            "package main\n\nfunc Total(items []int) int {\n\treturn 0\n}",
        ),
    ];

    for (lang, code) in snippets {
        let mut parser = lang
            .create_parser()
            .unwrap_or_else(|e| panic!("Failed to create parser for {lang:?}: {e}"));
        let tree = parser.parse(code, None);
        assert!(tree.is_some(), "Parser returned None for {lang:?}");
        let tree = tree.unwrap();
        let root = tree.root_node();
        assert!(root.child_count() > 0, "Root has no children for {lang:?}");
    }
}

#[test]
fn test_create_parser_standalone() {
    let lang = detect_language(Path::new("test.rs")).unwrap();
    let mut parser = create_parser(&lang).expect("Failed to create standalone parser");
    let tree = parser.parse("struct Foo;", None).unwrap();
    assert_eq!(tree.root_node().kind(), "source_file");
}

#[test]
fn test_error_recovery_on_incomplete_code() {
    let mut parser = SupportedLanguage::Rust.create_parser().unwrap();
    let tree = parser.parse("fn unclosed(", None);
    assert!(tree.is_some());
}
