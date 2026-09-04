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
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;

pub const MAX_RG_FILE_BYTES: u64 = 1_000_000;

pub struct RgQuery {
    pub workspace_root: PathBuf,
    pub search_root: PathBuf,
    pub matcher: RegexMatcher,
    pub types: Option<Types>,
    pub include_hidden: bool,
}

impl RgQuery {
    pub fn run(self, limit: usize) -> ToolResult {
        let RgQuery {
            workspace_root,
            search_root,
            matcher,
            types,
            include_hidden,
        } = self;
        let mut builder = walker_builder(&search_root, include_hidden);
        if let Some(types) = &types {
            builder.types(types.clone());
        }

        let matches: Mutex<Vec<LineMatch>> = Mutex::new(Vec::new());
        builder.build_parallel().run(|| {
            let mut searcher = SearcherBuilder::new()
                .line_number(true)
                .binary_detection(BinaryDetection::quit(b'\x00'))
                .build();
            // Shared state is captured by reference; the searcher is owned by
            // each visitor so the boxed closure stays self-contained.
            let matches = &matches;
            let matcher = &matcher;
            let workspace_root = &workspace_root;
            Box::new(move |entry| {
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                // The walker's type matcher only filters files, so directory
                // entries still arrive here and must be excluded from search.
                let Some(file_type) = entry.file_type() else {
                    return WalkState::Continue;
                };
                if file_type.is_dir() || file_type.is_symlink() {
                    return WalkState::Continue;
                }
                let Ok(relative) = entry.path().strip_prefix(workspace_root) else {
                    return WalkState::Continue;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                if matches.lock().unwrap_or_else(PoisonError::into_inner).len() >= RG_COLLECTION_CEILING {
                    return WalkState::Quit;
                }
                let Ok(metadata) = entry.metadata() else {
                    return WalkState::Continue;
                };
                if metadata.len() > MAX_RG_FILE_BYTES {
                    return WalkState::Continue;
                }
                let mut sink = UTF8(|line_number, line| {
                    let mut matches = matches.lock().unwrap_or_else(PoisonError::into_inner);
                    if matches.len() >= RG_COLLECTION_CEILING {
                        return Ok(false); // stop matching this file; the Quit below follows
                    }
                    // Truncate at collection time so pathological one-line files
                    // cannot balloon shared state; pi computes the same text at
                    // render time, and the flag below only counts shown rows.
                    let truncated = truncate_line(line.trim_end_matches(['\n', '\r']));
                    matches.push(LineMatch {
                        path: relative.clone(),
                        line: line_number,
                        text: truncated.text,
                        truncated: truncated.was_truncated,
                    });
                    Ok(true)
                });
                // Unreadable files are skipped, never fatal.
                if searcher.search_path(matcher, entry.path(), &mut sink).is_err() {
                    return WalkState::Continue;
                }
                if matches.lock().unwrap_or_else(PoisonError::into_inner).len() >= RG_COLLECTION_CEILING {
                    WalkState::Quit
                } else {
                    WalkState::Continue
                }
            })
        });

        let matches = matches.into_inner().unwrap_or_else(PoisonError::into_inner);
        format_results(matches, limit)
    }
}
