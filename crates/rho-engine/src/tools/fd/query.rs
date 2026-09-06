use super::entry::{FD_COLLECTION_CEILING, FdEntry, FdFormat, format_results, sort_entries};
use super::stats::{FileStats, count_file_stats};
use crate::tools::traversal::walker_builder;
use crate::tools::types::ToolResult;
use ignore::WalkState;
use ignore::types::Types;
use regex::Regex;
use rho_harness_core::args::FdSort;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct FdQuery {
    pub workspace_root: PathBuf,
    pub search_root: PathBuf,
    pub search_path_display: Option<String>,
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

fn resolve_entry_relative_path(
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

fn check_stats_lines(path: &Path, min: Option<usize>, max: Option<usize>) -> Option<Option<FileStats>> {
    let s = count_file_stats(path);
    if min.is_some_and(|m| s.map_or(0, |st| st.lines) < m) {
        return None;
    }
    if max.is_some_and(|m| s.map_or(0, |st| st.lines) > m) {
        return None;
    }
    Some(s)
}

fn push_entry_under_ceiling(collected: &Mutex<Vec<FdEntry>>, hit_ceiling: &AtomicBool, entry: FdEntry) -> WalkState {
    let mut entries = collected.lock().unwrap_or_else(PoisonError::into_inner);
    if entries.len() >= FD_COLLECTION_CEILING {
        return WalkState::Quit;
    }
    entries.push(entry);
    if entries.len() >= FD_COLLECTION_CEILING {
        hit_ceiling.store(true, Ordering::Relaxed);
        return WalkState::Quit;
    }
    WalkState::Continue
}

fn setup_walker_builder(
    (search_root, depth): (&Path, Option<usize>),
    types: Option<&Types>,
    include_hidden: bool,
) -> ignore::WalkBuilder {
    let mut builder = walker_builder(search_root, include_hidden);
    builder.max_depth(depth);
    if let Some(types) = types {
        builder.types(types.clone());
    }
    builder
}

type FdWalkFilters<'a> = (
    Option<&'a Regex>,
    Option<&'a Types>,
    Option<&'a str>,
    Option<usize>,
    Option<usize>,
    bool,
);

fn process_walk_entry(
    entry: ignore::DirEntry,
    (w_root, s_root): (&Path, &Path),
    (reg, ty, display, min_lines, max_lines, stats_needed): FdWalkFilters<'_>,
) -> Option<FdEntry> {
    let relative = resolve_entry_relative_path(entry.path(), (w_root, s_root), display);
    if relative.is_empty() || reg.is_some_and(|r| !r.is_match(&relative)) {
        return None;
    }
    let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
    if is_dir && (ty.is_some() || min_lines.is_some() || max_lines.is_some()) {
        return None;
    }
    let stats = if stats_needed && !is_dir {
        check_stats_lines(entry.path(), min_lines, max_lines)?
    } else {
        None
    };
    Some(FdEntry {
        relative,
        is_dir,
        stats,
    })
}

impl FdQuery {
    fn run_traversal(
        &self,
        builder: ignore::WalkBuilder,
        (collected, hit_ceiling): (&Mutex<Vec<FdEntry>>, &AtomicBool),
    ) {
        let (s_root, w_root) = (self.search_root.as_path(), self.workspace_root.as_path());
        let (reg, ty) = (self.regex.as_ref(), self.types.as_ref());
        let display = self.search_path_display.as_deref();
        builder.build_parallel().run(|| {
            Box::new(|entry| {
                let Ok(entry) = entry else { return WalkState::Continue };
                let filters = (reg, ty, display, self.min_lines, self.max_lines, self.stats_needed);
                let Some(fd_entry) = process_walk_entry(entry, (w_root, s_root), filters) else {
                    return WalkState::Continue;
                };
                push_entry_under_ceiling(collected, hit_ceiling, fd_entry)
            })
        });
    }

    pub fn run(self, limit: usize) -> ToolResult {
        let builder = setup_walker_builder(
            (&self.search_root, self.depth),
            self.types.as_ref(),
            self.include_hidden,
        );
        let collected: Mutex<Vec<FdEntry>> = Mutex::new(Vec::new());
        let hit_ceiling = AtomicBool::new(false);
        self.run_traversal(builder, (&collected, &hit_ceiling));

        let mut entries = collected.into_inner().unwrap_or_else(PoisonError::into_inner);
        sort_entries(&mut entries, self.sort);
        format_results(
            entries,
            FdFormat {
                hit_ceiling: hit_ceiling.load(Ordering::Relaxed),
                limit,
                show_stats: self.show_stats,
            },
        )
    }
}
