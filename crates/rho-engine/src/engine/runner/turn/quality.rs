use ripwire_rs::domain::CorpusView;
use ripwire_rs::gate::{
    QualityDeltaReport, SignatureContract, check_symbol_contract, evaluate_quality_delta, find_affected_tests,
    load_head_symbol_contract, summarize_test_gate,
};
use ripwire_rs::query::FileSources;
use ripwire_rs::workspace::Workspace;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegressionNotice {
    pub kind: String,
    pub sym_name: String,
    pub file_path: String,
    pub line: u32,
    pub was_val: u32,
    pub now_val: u32,
    pub is_new_symbol: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractNotice {
    pub symbol_name: String,
    pub file_path: String,
    pub line: u32,
    pub incompatible_callers: usize,
    pub caller_summaries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntestedNotice {
    pub untested_count: usize,
    pub top_untested: Vec<(String, String, u32, u32)>,
    pub affected_tests: Vec<String>,
}

fn check_dirty_files(root: &str) -> Option<Vec<String>> {
    let changes = ripwire_rs::git::repo::working_tree_changes(root);
    if changes.is_empty() { None } else { Some(changes) }
}

fn collect_quality_regressions(
    root: &str,
    query_ctx: &ripwire_rs::query::commands::QueryContext<'_>,
) -> Vec<RegressionNotice> {
    let Ok(rep) = evaluate_quality_delta(root, query_ctx) else {
        return Vec::new();
    };
    filter_regressions(&rep)
}

fn filter_regressions(rep: &QualityDeltaReport) -> Vec<RegressionNotice> {
    rep.regressions
        .iter()
        .filter(|r| r.is_gating || (r.is_new_symbol && !r.is_minor))
        .map(|r| RegressionNotice {
            kind: r.kind.clone(),
            sym_name: r.sym_name.clone(),
            file_path: r.file_path.clone(),
            line: r.line,
            was_val: r.was_val,
            now_val: r.now_val,
            is_new_symbol: r.is_new_symbol,
        })
        .collect()
}

fn collect_contract_breaks(workspace: &Workspace, changed_files: &[String]) -> Vec<ContractNotice> {
    let view = (&workspace.ingest, &workspace.interner);
    let mut notices = Vec::new();

    for sym in &workspace.ingest.symbols {
        let path = view.sym_path(sym);
        if !changed_files.iter().any(|f| path == f || path.ends_with(f)) {
            continue;
        }

        let sym_name = view.sym_name(sym);
        let head_contract = load_head_symbol_contract(&workspace.root, path, sym_name, sym.lang)
            .ok()
            .flatten();

        if let Some(notice) = check_contract_for_symbol(workspace, sym.id, head_contract.as_ref()) {
            notices.push(notice);
        }
    }

    notices
}

fn check_contract_for_symbol(
    workspace: &Workspace,
    sym_id: ripwire_rs::domain::NodeId,
    head_contract: Option<&SignatureContract>,
) -> Option<ContractNotice> {
    let report = check_symbol_contract(
        sym_id,
        head_contract,
        &workspace.ingest,
        &workspace.graph,
        &workspace.interner,
    )?;

    if report.incompatible_callers == 0 {
        return None;
    }

    let caller_summaries = report
        .callers
        .iter()
        .filter(|c| c.is_incompatible)
        .take(3)
        .map(|c| {
            let site_info = c
                .sites
                .first()
                .map(|s| {
                    format!(
                        " at line {}, passing {} args (requires {})",
                        s.line, s.arg_count, s.required_params
                    )
                })
                .unwrap_or_default();
            format!("`{}` in `{}`{}", c.caller_name, c.file_path, site_info)
        })
        .collect();

    Some(ContractNotice {
        symbol_name: report.symbol_name,
        file_path: report.file_path,
        line: report.line,
        incompatible_callers: report.incompatible_callers,
        caller_summaries,
    })
}

fn collect_untested_impact(workspace: &Workspace, changed_files: &[String]) -> Option<UntestedNotice> {
    let specs: Vec<&str> = changed_files.iter().map(String::as_str).collect();
    let report = find_affected_tests(&specs, &workspace.ingest, &workspace.graph, &workspace.interner);

    let tested =
        ripwire_rs::navigate::compute_tested_node_mask(&workspace.ingest, &workspace.graph, &workspace.interner);
    let summary = summarize_test_gate(&report, &tested, &workspace.ingest, &workspace.interner);

    if summary.untested.is_empty() || report.tests.is_empty() {
        return None;
    }

    let view = (&workspace.ingest, &workspace.interner);
    let top_untested: Vec<(String, String, u32, u32)> = summary
        .untested
        .iter()
        .take(4)
        .map(|&id| {
            let sym = &workspace.ingest[id];
            (
                view.sym_name(sym).to_string(),
                view.sym_path(sym).to_string(),
                sym.line,
                sym.ccx,
            )
        })
        .collect();

    let affected_tests: Vec<String> = report.tests.iter().take(3).map(|t| t.path.clone()).collect();

    Some(UntestedNotice {
        untested_count: summary.untested.len(),
        top_untested,
        affected_tests,
    })
}

fn format_regression_entry(r: &RegressionNotice) -> String {
    let origin_label = if r.is_new_symbol {
        "New symbol exceeds quality bar"
    } else {
        "Quality regression"
    };

    let metric_desc = match r.kind.as_str() {
        "complexity" => format!(
            "Cognitive Complexity: `{}` in `{}:{}` (score was {}, now {}, threshold is <= 15). Action: Invert conditionals or extract inner logic into private helpers.",
            r.sym_name, r.file_path, r.line, r.was_val, r.now_val
        ),
        "nesting" => format!(
            "Deep Nesting: `{}` in `{}:{}` (nesting is {}, threshold is <= 4). Action: Invert conditionals with early returns.",
            r.sym_name, r.file_path, r.line, r.now_val
        ),
        "params" => format!(
            "Parameter Count: `{}` in `{}:{}` ({} parameters, threshold is <= 5). Action: Bundle related arguments into a context/config struct.",
            r.sym_name, r.file_path, r.line, r.now_val
        ),
        "verbosity" => format!(
            "Function Length: `{}` in `{}:{}` (length is {} lines). Action: Break oversized function into smaller, cohesive units.",
            r.sym_name, r.file_path, r.line, r.now_val
        ),
        "dead-code" => format!(
            "Dead Code: `{}` in `{}:{}` has no reachable callers or references. Action: Wire up to intended caller or remove.",
            r.sym_name, r.file_path, r.line
        ),
        "duplication" => format!(
            "Code Duplication: clone detected for `{}` in `{}:{}`. Action: Consolidate shared logic.",
            r.sym_name, r.file_path, r.line
        ),
        other => format!(
            "{}: `{}` in `{}:{}` (was {}, now {}). Action: Inspect and resolve before completion.",
            other, r.sym_name, r.file_path, r.line, r.was_val, r.now_val
        ),
    };

    format!("• [{origin_label}] {metric_desc}")
}

fn append_regressions_section(out: &mut String, regressions: &[RegressionNotice]) {
    if regressions.is_empty() {
        return;
    }
    out.push_str("### Code Quality Regressions:\n");
    for r in regressions {
        out.push_str(&format_regression_entry(r));
        out.push('\n');
    }
    out.push('\n');
}

fn append_contracts_section(out: &mut String, contracts: &[ContractNotice]) {
    if contracts.is_empty() {
        return;
    }
    out.push_str("### Broken Contracts (Incompatible Callers):\n");
    for c in contracts {
        out.push_str(&format!(
            "• Symbol `{}` in `{}:{}` broke {} caller(s):\n",
            c.symbol_name, c.file_path, c.line, c.incompatible_callers
        ));
        for caller in &c.caller_summaries {
            out.push_str(&format!("  - {caller}\n"));
        }
        out.push_str("  Action: Update broken callsites or keep signature backwards-compatible.\n");
    }
    out.push('\n');
}

fn append_untested_section(out: &mut String, untested: Option<&UntestedNotice>) {
    let Some(u) = untested else {
        return;
    };
    out.push_str("### Untested Blast Radius:\n");
    out.push_str(&format!(
        "• {} impacted symbol(s) have zero test reach:\n",
        u.untested_count
    ));
    for (name, path, line, ccx) in &u.top_untested {
        out.push_str(&format!("  - `{name}` in `{path}:{line}` (ccx: {ccx})\n"));
    }
    if !u.affected_tests.is_empty() {
        out.push_str("  Affected test files to run/cover:\n");
        for test in &u.affected_tests {
            out.push_str(&format!("  - `{test}`\n"));
        }
    }
    out.push_str("  Action: Add unit test coverage or run the affected tests.\n\n");
}

fn format_rejection_feedback(
    regressions: &[RegressionNotice],
    contracts: &[ContractNotice],
    untested: Option<&UntestedNotice>,
) -> Option<String> {
    if regressions.is_empty() && contracts.is_empty() && untested.is_none() {
        return None;
    }

    let mut out = String::from(
        "[Static Quality Gate Rejection - Action Required]\nYour task cannot be marked completed yet because static code analysis detected blocking regressions or issues in your changes:\n\n",
    );

    append_regressions_section(&mut out, regressions);
    append_contracts_section(&mut out, contracts);
    append_untested_section(&mut out, untested);

    out.push_str("Please address these issues before declaring your work complete.");
    Some(out)
}

/// Run in-process ripwire-rs quality gate verification on the working tree.
/// Returns Some(feedback_prompt) if gating issues exist, or None if clean.
pub fn check_completion_quality(working_dir: &Path) -> Option<String> {
    let root_str = working_dir.to_str()?;
    let dirty_files = check_dirty_files(root_str)?;

    let workspace = Workspace::load_uncached(root_str)?;
    let sources = FileSources {
        root: Path::new(root_str),
    };
    let query_ctx = workspace.query_context(&sources);

    let regressions = collect_quality_regressions(root_str, &query_ctx);
    let contracts = collect_contract_breaks(&workspace, &dirty_files);
    let untested = collect_untested_impact(&workspace, &dirty_files);

    format_rejection_feedback(&regressions, &contracts, untested.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_regression_entry_complexity() {
        let entry = RegressionNotice {
            kind: "complexity".into(),
            sym_name: "handle_turn".into(),
            file_path: "src/turn.rs".into(),
            line: 42,
            was_val: 10,
            now_val: 22,
            is_new_symbol: false,
        };
        let formatted = format_regression_entry(&entry);
        assert!(formatted.contains("Cognitive Complexity"));
        assert!(formatted.contains("handle_turn"));
        assert!(formatted.contains("was 10, now 22"));
    }

    #[test]
    fn test_format_rejection_feedback_empty_when_clean() {
        let feedback = format_rejection_feedback(&[], &[], None);
        assert!(feedback.is_none());
    }

    #[test]
    fn test_format_rejection_feedback_with_regressions_and_contracts() {
        let regressions = vec![RegressionNotice {
            kind: "nesting".into(),
            sym_name: "deep_func".into(),
            file_path: "src/deep.rs".into(),
            line: 12,
            was_val: 2,
            now_val: 6,
            is_new_symbol: true,
        }];
        let contracts = vec![ContractNotice {
            symbol_name: "public_api".into(),
            file_path: "src/api.rs".into(),
            line: 15,
            incompatible_callers: 1,
            caller_summaries: vec!["`legacy_caller` in `src/main.rs:10`".into()],
        }];
        let untested = UntestedNotice {
            untested_count: 1,
            top_untested: vec![("calc".into(), "src/calc.rs".into(), 20, 8)],
            affected_tests: vec!["tests/calc_test.rs".into()],
        };

        let feedback = format_rejection_feedback(&regressions, &contracts, Some(&untested));
        assert!(feedback.is_some());
        let text = feedback.unwrap();
        assert!(text.contains("Static Quality Gate Rejection"));
        assert!(text.contains("Deep Nesting"));
        assert!(text.contains("Broken Contracts"));
        assert!(text.contains("Untested Blast Radius"));
    }

    #[test]
    fn test_check_dirty_files_clean_on_clean_dir() {
        assert!(check_dirty_files("/nonexistent_path_42").is_none());
    }
}
