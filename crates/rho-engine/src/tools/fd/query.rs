use super::entry::{FD_COLLECTION_CEILING, FdEntry, FdFormat, format_results, sort_entries};
use super::stats::count_file_stats;
use crate::tools::traversal::walker_builder;
use crate::tools::types::ToolResult;
use ignore::WalkState;
use ignore::types::Types;
use regex::Regex;
use rho_harness_core::args::FdSort;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct FdQuery {
    pub workspace_root: PathBuf,
    pub search_root: PathBuf,
    pub regex: Option<Regex>,
    pub types: Option<Types>,
    pub include_hidden: bool,
    pub depth: Option<usize>,
    pub stats_needed: bool,
    pub min_lines: Option<usize>,
    pub max_lines: Option<usize>,
    pub sort: Option<FdSort>,
    pub show_stats: bool,
}

impl FdQuery {
    pub fn run(self, limit: usize) -> ToolResult {
        let FdQuery {
            workspace_root,
            search_root,
            regex,
            types,
            include_hidden,
            depth,
            stats_needed,
            min_lines,
            max_lines,
            sort,
            show_stats,
        } = self;
        let mut builder = walker_builder(&search_root, include_hidden);
        builder.max_depth(depth);
        if let Some(types) = &types {
            builder.types(types.clone());
        }

        let collected: Mutex<Vec<FdEntry>> = Mutex::new(Vec::new());
        let hit_ceiling = AtomicBool::new(false);
        builder.build_parallel().run(|| {
            Box::new(|entry| {
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                let Ok(relative) = entry.path().strip_prefix(&workspace_root) else {
                    return WalkState::Continue;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                if relative.is_empty() || regex.as_ref().is_some_and(|r| !r.is_match(&relative)) {
                    return WalkState::Continue;
                }
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                if types.is_some() && is_dir {
                    return WalkState::Continue;
                }
                if is_dir && (min_lines.is_some() || max_lines.is_some()) {
                    return WalkState::Continue;
                }

                let stats = if stats_needed && !is_dir {
                    let s = count_file_stats(entry.path());
                    if min_lines.is_some_and(|min| s.map_or(0, |st| st.lines) < min) {
                        return WalkState::Continue;
                    }
                    if max_lines.is_some_and(|max| s.map_or(0, |st| st.lines) > max) {
                        return WalkState::Continue;
                    }
                    s
                } else {
                    None
                };

                let mut entries = collected.lock().unwrap_or_else(PoisonError::into_inner);
                if entries.len() >= FD_COLLECTION_CEILING {
                    return WalkState::Quit;
                }
                entries.push(FdEntry {
                    relative,
                    is_dir,
                    stats,
                });
                if entries.len() >= FD_COLLECTION_CEILING {
                    hit_ceiling.store(true, Ordering::Relaxed);
                    return WalkState::Quit;
                }
                WalkState::Continue
            })
        });

        let mut entries = collected.into_inner().unwrap_or_else(PoisonError::into_inner);
        sort_entries(&mut entries, sort);
        format_results(
            entries,
            FdFormat {
                hit_ceiling: hit_ceiling.load(Ordering::Relaxed),
                limit,
                show_stats,
            },
        )
    }
}
