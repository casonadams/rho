use serde::Serialize;
use std::collections::BTreeMap;

const MIN_SCOPE_FILES: usize = 40;
const LARGE_REPO_FILES: usize = 2_000;
const NON_REVIEWABLE_EXTENSIONS: [&str; 30] = [
    "png", "jpg", "jpeg", "gif", "svg", "ico", "webp", "bmp", "woff", "woff2", "ttf", "otf", "eot", "mp3", "mp4",
    "mov", "wav", "zip", "gz", "tgz", "tar", "7z", "pdf", "bin", "lock", "map", "md", "mdx", "txt", "rst",
];

#[must_use]
pub fn reviewable_dir_counts(files: &[String]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for file in files {
        let name = file.rsplit('/').next().unwrap_or(file);
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        if ext.is_some_and(|e| NON_REVIEWABLE_EXTENSIONS.contains(&e.as_str())) {
            continue;
        }
        let dir = file.rsplit_once('/').map_or(".", |(d, _)| d);
        *counts.entry(dir.to_string()).or_insert(0) += 1;
    }
    counts
}

#[must_use]
pub fn default_max_scopes(reviewable_files: usize) -> usize {
    if reviewable_files > LARGE_REPO_FILES { 5 } else { 3 }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scope {
    pub paths: Vec<String>,
    pub files: usize,
}

#[derive(Default)]
struct Node {
    path: String,
    own: usize,
    total: usize,
    children: BTreeMap<String, Node>,
}

/// Splits per-directory file counts into at most `max_scopes` disjoint slices of roughly equal size.
/// A path `dir` is a whole subtree, `dir/*` only the files directly in `dir`, `.` the repository root.
#[must_use]
pub fn plan_scopes(dir_counts: &BTreeMap<String, usize>, max_scopes: usize) -> Vec<Scope> {
    let max_scopes = max_scopes.max(1);
    let root = build_tree(dir_counts);
    if root.total == 0 {
        return Vec::new();
    }
    let target = MIN_SCOPE_FILES.max(root.total.div_ceil(max_scopes));
    let mut scopes = collect(&root, target);
    while scopes.len() > max_scopes {
        merge_smallest(&mut scopes);
    }
    scopes.sort_by(by_size);
    scopes
}

fn by_size(a: &Scope, b: &Scope) -> std::cmp::Ordering {
    b.files.cmp(&a.files).then_with(|| a.paths[0].cmp(&b.paths[0]))
}

fn whole(node: &Node) -> Scope {
    Scope {
        paths: vec![node.path.clone()],
        files: node.total,
    }
}

fn collect(node: &Node, target: usize) -> Vec<Scope> {
    if node.total <= target || node.children.is_empty() {
        return vec![whole(node)];
    }
    let mut scopes = Vec::new();
    let mut small = Vec::new();
    for child in node.children.values() {
        if child.total > target {
            scopes.extend(collect(child, target));
        } else {
            small.push(whole(child));
        }
    }
    if node.own > 0 {
        let path = if node.path == "." {
            "./*".to_string()
        } else {
            format!("{}/*", node.path)
        };
        let direct = Scope {
            paths: vec![path],
            files: node.own,
        };
        if direct.files > target {
            scopes.push(direct);
        } else {
            small.push(direct);
        }
    }
    scopes.extend(pack(small, target));
    scopes
}

fn pack(mut pieces: Vec<Scope>, target: usize) -> Vec<Scope> {
    pieces.sort_by(by_size);
    let mut bins: Vec<Scope> = Vec::new();
    for piece in pieces {
        match bins.iter_mut().find(|bin| bin.files + piece.files <= target) {
            Some(bin) => {
                bin.paths.extend(piece.paths);
                bin.files += piece.files;
            }
            None => bins.push(piece),
        }
    }
    bins
}

fn merge_smallest(scopes: &mut Vec<Scope>) {
    scopes.sort_by(|a, b| by_size(b, a));
    let first = scopes.remove(0);
    let second = scopes.remove(0);
    let mut paths = first.paths;
    paths.extend(second.paths);
    scopes.push(Scope {
        paths,
        files: first.files + second.files,
    });
}

fn build_tree(dir_counts: &BTreeMap<String, usize>) -> Node {
    let mut root = Node {
        path: ".".to_string(),
        ..Node::default()
    };
    for (dir, &count) in dir_counts {
        let mut node = &mut root;
        node.total += count;
        for segment in dir.split('/').filter(|s| !s.is_empty() && *s != ".") {
            let path = if node.path == "." {
                segment.to_string()
            } else {
                format!("{}/{segment}", node.path)
            };
            node = node.children.entry(segment.to_string()).or_insert_with(|| Node {
                path,
                ..Node::default()
            });
            node.total += count;
        }
        node.own += count;
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
        entries.iter().map(|(d, c)| ((*d).to_string(), *c)).collect()
    }

    #[test]
    fn counts_only_reviewable_files_per_directory() {
        let files: Vec<String> = [
            "Cargo.toml",
            "Cargo.lock",
            "README.md",
            "src/lib.rs",
            "src/ui/a.rs",
            "www/logo.PNG",
        ]
        .iter()
        .map(|f| (*f).to_string())
        .collect();
        assert_eq!(
            reviewable_dir_counts(&files),
            counts(&[(".", 1), ("src", 1), ("src/ui", 1)])
        );
        assert_eq!(default_max_scopes(10), 3);
        assert_eq!(default_max_scopes(5_000), 5);
    }

    #[test]
    fn small_repo_is_one_scope() {
        let scopes = plan_scopes(&counts(&[(".", 3), ("src", 10)]), 3);
        assert_eq!(
            scopes,
            vec![Scope {
                paths: vec![".".into()],
                files: 13
            }]
        );
    }

    #[test]
    fn empty_repo_has_no_scopes() {
        assert!(plan_scopes(&BTreeMap::new(), 3).is_empty());
    }

    #[test]
    fn splits_large_trees_into_disjoint_bounded_scopes() {
        let dirs = counts(&[
            (".", 5),
            ("src", 60),
            ("src/ui", 80),
            ("crates/a", 70),
            ("crates/b", 70),
            ("docs", 2),
        ]);
        let scopes = plan_scopes(&dirs, 3);
        assert!(scopes.len() <= 3);
        assert_eq!(scopes.iter().map(|s| s.files).sum::<usize>(), 287);
        let mut all: Vec<&String> = scopes.iter().flat_map(|s| &s.paths).collect();
        let before = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), before, "paths overlap: {scopes:?}");
        assert!(scopes.windows(2).all(|w| w[0].files >= w[1].files));
    }

    #[test]
    fn direct_files_get_star_paths() {
        let dirs = counts(&[(".", 50), ("a", 50), ("b", 50)]);
        let scopes = plan_scopes(&dirs, 3);
        let paths: Vec<&str> = scopes.iter().flat_map(|s| s.paths.iter().map(String::as_str)).collect();
        assert!(paths.contains(&"./*"), "{paths:?}");
    }
}
