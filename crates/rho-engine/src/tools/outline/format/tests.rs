use super::*;
use crate::tools::outline::types::SymbolKind;

#[test]
fn test_format_outlines_clean_hierarchical_lines() {
    let outlines = vec![FileOutline {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            SymbolEntry {
                name: "AgentEngine".to_string(),
                kind: SymbolKind::Struct,
                signature: "pub struct AgentEngine".to_string(),
                line: 10,
                depth: 0,
            },
            SymbolEntry {
                name: "AgentEngine".to_string(),
                kind: SymbolKind::Impl,
                signature: "impl AgentEngine".to_string(),
                line: 20,
                depth: 0,
            },
            SymbolEntry {
                name: "new".to_string(),
                kind: SymbolKind::Method,
                signature: "pub fn new() -> Self".to_string(),
                line: 22,
                depth: 1,
            },
        ],
    }];

    let res = format_outlines(&outlines, false);
    assert!(!res.is_error);
    let expected = "\
src/lib.rs:
  line 10: pub struct AgentEngine
  line 20: impl AgentEngine
    line 22: pub fn new() -> Self";
    assert_eq!(res.content, expected);
}

#[test]
fn test_format_outlines_empty() {
    let res = format_outlines(&[], false);
    assert!(!res.is_error);
    assert_eq!(res.content, "No matching symbols found");

    let empty_file = vec![FileOutline {
        path: "src/lib.rs".to_string(),
        symbols: vec![],
    }];
    let res2 = format_outlines(&empty_file, false);
    assert!(!res2.is_error);
    assert_eq!(res2.content, "No matching symbols found");
}

#[test]
fn test_format_outlines_hit_file_limit() {
    let outlines = vec![FileOutline {
        path: "src/lib.rs".to_string(),
        symbols: vec![SymbolEntry {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            signature: "pub fn foo()".to_string(),
            line: 1,
            depth: 0,
        }],
    }];
    let res = format_outlines(&outlines, true);
    assert!(!res.is_error);
    assert!(res.content.contains("[scanned 500 files limit reached"));
}

#[test]
fn test_format_outlines_byte_truncation() {
    let mut symbols = Vec::new();
    // Generate enough symbols to exceed 50 KB
    for i in 0..1500 {
        symbols.push(SymbolEntry {
            name: format!("function_with_a_fairly_long_name_number_{i}"),
            kind: SymbolKind::Function,
            signature: format!(
                "pub fn function_with_a_fairly_long_name_number_{i}(param_a: String, param_b: i32) -> Result<String, ()>"
            ),
            line: i + 1,
            depth: 0,
        });
    }
    let outlines = vec![FileOutline {
        path: "src/huge.rs".to_string(),
        symbols,
    }];
    let res = format_outlines(&outlines, false);
    assert!(!res.is_error);
    assert!(res.content.contains("limit reached]"));
    assert!(res.content.len() <= DEFAULT_MAX_BYTES + 200);
}
