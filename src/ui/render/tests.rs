//! Tests for the `ui::render` module.

use super::formatters::{format_edit_diff, format_thinking_block, format_write_preview};
use super::summary::{bash_approval_details, clean_command_paths, risk_badge, to_relative_path};
use super::types::BashApproval;
use crate::tools::bash_ast::RiskTier;
use crate::ui::theme::Theme;

#[test]
fn test_bash_approval_details_include_command_and_reasons() {
    let reasons = vec!["Writes output through file redirection".to_string()];
    let details = bash_approval_details(&BashApproval {
        command: "echo test > output.txt",
        tier: RiskTier::Mutating,
        reasons: &reasons,
    });

    assert_eq!(
        details,
        [
            "$ echo test > output.txt",
            "",
            "- Writes output through file redirection"
        ]
    );
}

#[test]
fn test_risk_badges() {
    assert_eq!(risk_badge(RiskTier::ReadOnly), "[READ ONLY]");
    assert_eq!(risk_badge(RiskTier::Mutating), "[MUTATING]");
    assert_eq!(risk_badge(RiskTier::HighRisk), "[HIGH RISK: DESTRUCTIVE ACTION]");
}

#[test]
fn test_to_relative_path() {
    let cwd = std::env::current_dir().unwrap();
    let abs = cwd.join("src/main.rs");
    let rel = to_relative_path(abs.to_str().unwrap());
    assert_eq!(rel, "src/main.rs");
}

#[test]
fn test_clean_command_paths() {
    let cwd = std::env::current_dir().unwrap();
    let cwd_str = cwd.to_str().unwrap();
    let cmd = format!("cat {cwd_str}/Cargo.toml");
    let cleaned = clean_command_paths(&cmd);
    assert_eq!(cleaned, "cat Cargo.toml");
}

#[test]
fn test_format_edit_diff_renders_removals_and_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "let x = 1;",
                "newText": "let x = 2;\nlet y = 3;"
            }
        ]
    });
    let diff = format_edit_diff(&args, &theme).unwrap();
    assert!(diff.contains("```diff"));
    assert!(diff.contains("- let x = 1;"));
    assert!(diff.contains("+ let x = 2;"));
    assert!(diff.contains("+ let y = 3;"));
    assert!(diff.contains("```"));
    assert!(diff.ends_with('\n'));
}

#[test]
fn test_format_write_preview_renders_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "test.py",
        "content": "def main():\n    print('hello')"
    });
    let preview = format_write_preview(&args, &theme).unwrap();
    assert!(preview.contains("```diff"));
    assert!(preview.contains("+ def main():"));
    assert!(preview.contains("+     print('hello')"));
    assert!(preview.contains("```"));
}

#[test]
fn test_format_thinking_block_renders_dimmed_with_trailing_breaks() {
    let theme = Theme::default();
    let formatted = format_thinking_block("analyzing the problem\nchecking tests", &theme);
    assert!(formatted.contains("analyzing the problem"));
    assert!(formatted.contains("checking tests"));
    assert!(!formatted.contains("┌─ Thinking"));
    assert!(formatted.ends_with("\n\n"));
}
