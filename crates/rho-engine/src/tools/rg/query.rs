use super::entry::{LineMatch, RG_COLLECTION_CEILING, format_results};
use crate::tools::traversal::walker_builder;
use crate::tools::truncate::truncate_line;
use crate::tools::types::ToolResult;
use grep_regex::RegexMatcher;
use grep_searcher::BinaryDetection;
use grep_searcher::SearcherBuilder;
use grep_searcher::sinks::UTF8;
use ignore::WalkState;
use ignore::types::Types;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::PoisonError;

pub const MAX_RG_FILE_BYTES: u64 = 1_000_000;

pub struct RgQuery {
    pub workspace_root: PathBuf,
    pub search_root: PathBuf,
    pub search_path_display: Option<String>,
    pub matcher: RegexMatcher,
    pub types: Option<Types>,
    pub include_hidden: bool,
}

fn resolve_rg_relative_path(
    path: &Path,
    (workspace_root, search_root): (&Path, &Path),
    search_path_display: Option<&str>,
) -> String {
    if let Ok(rel) = path.strip_prefix(workspace_root) {
        rel.to_string_lossy().replace('\\', "/")
    } else if let Ok(rel) = path.strip_prefix(search_root) {
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let base = search_path_display.unwrap_or("");
        if rel_str.is_empty() {
            base.to_string()
        } else if base.is_empty() || base.ends_with('/') {
            format!("{base}{rel_str}")
        } else {
            format!("{base}/{rel_str}")
        }
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn should_search_entry(entry: &ignore::DirEntry) -> bool {
    let Some(file_type) = entry.file_type() else {
        return false;
    };
    if file_type.is_dir() || file_type.is_symlink() {
        return false;
    }
    entry.metadata().map(|m| m.len() <= MAX_RG_FILE_BYTES).unwrap_or(false)
}

fn search_file(
    (searcher, matcher): (&mut grep_searcher::Searcher, &RegexMatcher),
    (path, relative): (&Path, &str),
    matches: &Mutex<Vec<LineMatch>>,
) {
    let mut file_matches = Vec::new();
    let mut sink = UTF8(|line_number, line| {
        let truncated = truncate_line(line.trim_end_matches(['\n', '\r']));
        file_matches.push(LineMatch {
            path: relative.to_string(),
            line: line_number,
            text: truncated.text,
            truncated: truncated.was_truncated,
        });
        Ok(file_matches.len() < RG_COLLECTION_CEILING)
    });
    let _ = searcher.search_path(matcher, path, &mut sink);
    if !file_matches.is_empty() {
        let mut list = matches.lock().unwrap_or_else(PoisonError::into_inner);
        let remaining = RG_COLLECTION_CEILING.saturating_sub(list.len());
        if remaining > 0 {
            file_matches.truncate(remaining);
            list.extend(file_matches);
        }
    }
}

impl RgQuery {
    fn run_traversal(&self, builder: ignore::WalkBuilder, matches: &Mutex<Vec<LineMatch>>) {
        let (w_root, s_root) = (self.workspace_root.as_path(), self.search_root.as_path());
        let matcher = &self.matcher;
        let search_path_display = self.search_path_display.as_deref();
        builder.build_parallel().run(|| {
            let mut searcher = SearcherBuilder::new()
                .line_number(true)
                .binary_detection(BinaryDetection::quit(b'\x00'))
                .build();
            Box::new(move |entry| {
                let Ok(entry) = entry else { return WalkState::Continue };
                if !should_search_entry(&entry) {
                    return WalkState::Continue;
                }
                if matches.lock().unwrap_or_else(PoisonError::into_inner).len() >= RG_COLLECTION_CEILING {
                    return WalkState::Quit;
                }
                let relative = resolve_rg_relative_path(entry.path(), (w_root, s_root), search_path_display);
                search_file((&mut searcher, matcher), (entry.path(), &relative), matches);
                if matches.lock().unwrap_or_else(PoisonError::into_inner).len() >= RG_COLLECTION_CEILING {
                    WalkState::Quit
                } else {
                    WalkState::Continue
                }
            })
        });
    }

    pub fn run(self, limit: usize) -> ToolResult {
        let mut builder = walker_builder(&self.search_root, self.include_hidden);
        if let Some(types) = &self.types {
            builder.types(types.clone());
        }

        let matches = Mutex::new(Vec::new());
        self.run_traversal(builder, &matches);
        let list = matches.into_inner().unwrap_or_else(PoisonError::into_inner);
        format_results(list, limit)
    }
}
