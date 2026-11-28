//! "Did you mean" suggestions for paths that do not exist, ranked by name similarity.

use super::traversal::walker_builder;
use rho_harness_core::workspace::{canonicalize_path, clean_path_arg, expand_home};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

const MAX_ENTRIES: usize = 20_000;
const WALK_BUDGET: Duration = Duration::from_millis(250);
const MAX_SUGGESTIONS: usize = 3;
const MAX_WANTED_CHARS: usize = 255;
const MODULE_STEMS: [&str; 3] = ["mod", "index", "__init__"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathKind {
    File,
    Any,
}

struct Entry {
    path: String,
    is_dir: bool,
}

fn workspace_entries(root: &Path, kind: PathKind) -> Vec<Entry> {
    let deadline = Instant::now() + WALK_BUDGET;
    walker_builder(root, false, None)
        .build()
        .flatten()
        .take_while(|_| Instant::now() < deadline)
        .filter_map(|entry| {
            let is_dir = entry.file_type()?.is_dir();
            let relative = entry
                .path()
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let wanted = !relative.is_empty() && (kind == PathKind::Any || !is_dir);
            wanted.then_some(Entry { path: relative, is_dir })
        })
        .take(MAX_ENTRIES)
        .collect()
}

fn stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// The name a path is known by: its stem, or its directory name for module files like `foo/mod.rs`.
fn display_name(path: &str, is_dir: bool) -> String {
    let mut parts = path.rsplit('/');
    let last = parts.next().unwrap_or_default();
    let name = if is_dir { last } else { stem(last) };
    match parts.next() {
        Some(parent) if !is_dir && MODULE_STEMS.contains(&name) => parent.to_lowercase(),
        _ => name.to_lowercase(),
    }
}

/// Optimal string alignment distance: insertions, deletions, substitutions, and adjacent swaps cost 1.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let width = b.len() + 1;
    let mut d: Vec<usize> = vec![0; (a.len() + 1) * width];
    for (j, cell) in d.iter_mut().take(width).enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        d[i * width] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[(i - 1) * width + j] + 1)
                .min(d[i * width + j - 1] + 1)
                .min(d[(i - 1) * width + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[(i - 2) * width + j - 2] + 1);
            }
            d[i * width + j] = best;
        }
    }
    d[a.len() * width + b.len()]
}

fn shared_leading_dirs(missing_dirs: &[&str], candidate: &str) -> usize {
    missing_dirs
        .iter()
        .zip(candidate.split('/'))
        .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
        .count()
}

/// `path` relative to `root`, also trying the canonical forms of both (e.g. macOS `/var` vs `/private/var`).
fn strip_root(path: &Path, root: &Path) -> Option<PathBuf> {
    let roots = [Some(root.to_path_buf()), root.canonicalize().ok()];
    let paths = [canonicalize_path(path), path.to_path_buf()];
    paths
        .iter()
        .find_map(|p| roots.iter().flatten().find_map(|r| p.strip_prefix(r).ok()))
        .map(Path::to_path_buf)
}

/// Workspace-relative form of `raw`, or `None` when it points outside the workspace.
fn relative_target(root: &Path, raw: &str) -> Option<String> {
    let expanded = expand_home(clean_path_arg(raw));
    let path = Path::new(&expanded);
    let relative = if path.is_absolute() {
        strip_root(path, root)?
    } else {
        path.to_path_buf()
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Field order is the sort order: closest spelling first, then shared directories, then exact file name.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    distance: usize,
    unshared_dirs: usize,
    inexact_name: bool,
    path_len: usize,
    path: String,
}

pub(crate) fn suggest_paths(root: &Path, raw: &str, kind: PathKind) -> Vec<String> {
    let Some(target) = relative_target(root, raw) else {
        return Vec::new();
    };
    let (target_dirs, target_last) = target.rsplit_once('/').unwrap_or(("", &target));
    let target_dirs: Vec<&str> = target_dirs.split('/').filter(|d| !d.is_empty()).collect();
    let wanted = display_name(&target, false);
    let wanted_len = wanted.chars().count();
    if wanted_len > MAX_WANTED_CHARS {
        return Vec::new();
    }
    let wanted_last = target_last.to_lowercase();
    let tolerance = (wanted_len / 4).max(1);

    let mut ranked: Vec<Rank> = workspace_entries(root, kind)
        .into_iter()
        .filter_map(|entry| {
            let name = display_name(&entry.path, entry.is_dir);
            if name.chars().count().abs_diff(wanted_len) > tolerance {
                return None;
            }
            let distance = edit_distance(&wanted, &name);
            (distance <= tolerance).then(|| {
                let shared = shared_leading_dirs(&target_dirs, &entry.path);
                let exact_name = entry
                    .path
                    .rsplit('/')
                    .next()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&wanted_last));
                Rank {
                    distance,
                    unshared_dirs: usize::MAX - shared,
                    inexact_name: !exact_name,
                    path_len: entry.path.len(),
                    path: entry.path,
                }
            })
        })
        .collect();
    ranked.sort();
    ranked.into_iter().take(MAX_SUGGESTIONS).map(|rank| rank.path).collect()
}

/// A trailing "Did you mean" line for a not-found error, or an empty string when nothing is close.
pub(crate) async fn missing_path_hint(root: &Path, raw: &str, kind: PathKind) -> String {
    let (root, raw) = (root.to_path_buf(), raw.to_string());
    match tokio::task::spawn_blocking(move || format_hint(&suggest_paths(&root, &raw, kind))).await {
        Ok(hint) => hint,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(_) => String::new(),
    }
}

fn format_hint(suggestions: &[String]) -> String {
    if suggestions.is_empty() {
        return String::new();
    }
    format!("\nDid you mean: {}?", suggestions.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    fn workspace(files: &[&str]) -> crate::test_support::TestDir {
        let dir = crate::test_support::TestDir::new();
        for file in files {
            crate::test_support::write_file(&dir.join(file), "");
        }
        dir
    }

    #[test_case("src/repl/input_reader.rs", PathKind::Any, &["src/repl/input_reader", "src/repl/input_reader/mod.rs"]; "module dir and file")]
    #[test_case("src/repl/input_reader.rs", PathKind::File, &["src/repl/input_reader/mod.rs"]; "module file only")]
    #[test_case("crates/engine/src/client.rs", PathKind::File, &["crates/engine/src/mcp/client.rs"]; "moved file")]
    #[test_case("crates/engine/src/mcp/runtim.rs", PathKind::File, &["crates/engine/src/mcp/runtime.rs"]; "typo")]
    #[test_case("./docs/configuraton.md", PathKind::File, &["docs/configuration.md"]; "dot prefix typo")]
    fn suggests_moved_renamed_and_module_paths(raw: &str, kind: PathKind, expected: &[&str]) {
        let dir = workspace(&[
            "src/repl/input_reader/mod.rs",
            "src/repl/input_reader/worker.rs",
            "crates/engine/src/mcp/client.rs",
            "crates/engine/src/mcp/runtime.rs",
            "docs/configuration.md",
        ]);
        assert_eq!(suggest_paths(&dir, raw, kind), expected);
    }

    #[tokio::test]
    async fn unrelated_or_outside_paths_get_no_suggestions() {
        let dir = workspace(&["src/main.rs"]);
        assert!(suggest_paths(&dir, "totally/unrelated_thing.py", PathKind::Any).is_empty());
        assert!(suggest_paths(&dir, "/etc/hosts", PathKind::Any).is_empty());
        assert_eq!(missing_path_hint(&dir, "nothing_close.rs", PathKind::File).await, "");
    }

    #[test]
    fn paths_escaping_the_workspace_get_no_suggestions() {
        let dir = workspace(&["src/main.rs", "main.rs"]);
        let escaping = dir.join("../src/main.rs");
        for raw in ["../src/main.rs", "src/../../main.rs", escaping.to_str().unwrap()] {
            assert!(suggest_paths(&dir, raw, PathKind::Any).is_empty(), "{raw}");
        }
        assert_eq!(
            suggest_paths(&dir, "lib/../src/man.rs", PathKind::File)[0],
            "src/main.rs"
        );
    }

    #[test]
    fn dotfile_typos_do_not_match_every_short_name() {
        let dir = workspace(&["a.rs", "src/b.rs"]);
        assert_eq!(display_name(".envrc", false), ".envrc");
        assert_eq!(display_name("dir/.config.toml", false), ".config");
        assert!(suggest_paths(&dir, ".envrx", PathKind::Any).is_empty());
    }

    #[test]
    fn overlong_names_get_no_suggestions() {
        let dir = workspace(&["src/main.rs"]);
        let raw = "m".repeat(100_000);
        assert!(suggest_paths(&dir, &raw, PathKind::Any).is_empty());
    }

    #[test]
    fn absolute_paths_match_through_symlinked_roots() {
        let dir = workspace(&["src/lib.rs"]);
        let canonical = dir.canonicalize().unwrap();
        let raw = dir.join("src/lb.rs");
        assert_eq!(
            suggest_paths(&canonical, raw.to_str().unwrap(), PathKind::File),
            ["src/lib.rs"]
        );
        let canonical_raw = canonical.join("src/lb.rs");
        assert_eq!(
            suggest_paths(&dir, canonical_raw.to_str().unwrap(), PathKind::File),
            ["src/lib.rs"]
        );
    }

    #[tokio::test]
    async fn absolute_paths_inside_the_workspace_are_matched_and_hints_are_formatted() {
        let dir = workspace(&["src/lib.rs"]);
        let raw = dir.join("src/lb.rs");
        assert_eq!(
            missing_path_hint(&dir, raw.to_str().unwrap(), PathKind::File).await,
            "\nDid you mean: src/lib.rs?"
        );
    }

    #[test]
    fn ignored_files_are_not_suggested() {
        let dir = workspace(&["target/debug/app.rs", "src/app.rs"]);
        std::fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir(dir.join(".git")).unwrap();
        assert_eq!(suggest_paths(&dir, "ap.rs", PathKind::File), ["src/app.rs"]);
    }

    #[test_case("", "", 0; "both empty")]
    #[test_case("abc", "abc", 0; "identical")]
    #[test_case("abc", "abd", 1; "substitution")]
    #[test_case("abc", "ab", 1; "deletion")]
    #[test_case("iu", "ui", 1; "swap")]
    #[test_case("recieve", "receive", 1; "swap in word")]
    #[test_case("kitten", "sitting", 3; "several edits")]
    fn edit_distance_counts_single_character_changes_and_swaps(a: &str, b: &str, expected: usize) {
        assert_eq!(edit_distance(a, b), expected);
    }
}
