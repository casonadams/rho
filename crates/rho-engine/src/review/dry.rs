//! Duplication detection: windows of added code that repeat elsewhere, and identical function bodies.

use super::functions;
use super::lang::Lang;
use super::source::{Line, Source};
use super::{Finding, Severity};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

const WINDOW: usize = 5;
const MIN_WINDOW_CHARS: usize = 100;
const MIN_FUNCTION_LINES: usize = 4;
const MIN_FUNCTION_CHARS: usize = 60;
const MAX_FINDINGS: usize = 50;

/// The existing code that added code is compared with; implemented over the checkout by adapters.
pub trait Corpus {
    /// `(path, text)` of every file of the language family `family`.
    fn files(&self, family: Lang) -> Vec<(String, String)>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    path: String,
    line: usize,
    idx: usize,
}

struct Sig {
    no: usize,
    norm: String,
    added: bool,
}

struct FnBody {
    name: String,
    first: usize,
    last: usize,
    hash: Option<u64>,
    added: bool,
}

struct Indexed {
    path: String,
    sig: Vec<Sig>,
    windows: Vec<Option<u64>>,
    fns: Vec<FnBody>,
}

fn normalized(line: &Line) -> Option<String> {
    let raw = line.raw.as_str();
    let boilerplate = line.comment
        || line.test
        || raw.chars().all(|c| matches!(c, '}' | ')' | ']' | ';' | ',' | ' '))
        || [
            "use ", "pub use ", "import ", "from ", "#include", "package ", "#[", "#![",
        ]
        .iter()
        .any(|p| raw.starts_with(p));
    (!boilerplate).then(|| raw.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn hash_all<'a>(items: impl Iterator<Item = &'a str>) -> u64 {
    let mut hasher = DefaultHasher::new();
    items.for_each(|s| s.hash(&mut hasher));
    hasher.finish()
}

fn chars_in(items: &[&str]) -> usize {
    items.iter().map(|s| s.len()).sum()
}

impl Indexed {
    fn new(source: &Source) -> Self {
        let sig: Vec<Sig> = source
            .lines
            .iter()
            .filter_map(|l| {
                normalized(l).map(|norm| Sig {
                    no: l.no,
                    norm,
                    added: l.added,
                })
            })
            .collect();
        let windows = sig
            .windows(WINDOW)
            .map(|w| {
                let norms: Vec<&str> = w.iter().map(|s| s.norm.as_str()).collect();
                (chars_in(&norms) >= MIN_WINDOW_CHARS).then(|| hash_all(norms.into_iter()))
            })
            .collect();
        let fns = functions::find(source)
            .iter()
            .map(|f| {
                let body = &source.lines[f.start + 1..=f.end];
                let norms: Vec<String> = body.iter().filter_map(normalized).collect();
                let refs: Vec<&str> = norms.iter().map(String::as_str).collect();
                let big = norms.len() >= MIN_FUNCTION_LINES && chars_in(&refs) >= MIN_FUNCTION_CHARS;
                FnBody {
                    name: f.name.clone(),
                    first: source.lines[f.start].no,
                    last: source.lines[f.end].no,
                    hash: big.then(|| hash_all(refs.into_iter())),
                    added: body.iter().any(|l| l.added),
                }
            })
            .collect();
        Self {
            path: source.path.clone(),
            sig,
            windows,
            fns,
        }
    }

    fn window_is_added(&self, i: usize) -> bool {
        self.sig[i..i + WINDOW].iter().all(|s| s.added)
    }

    fn target(&self, idx: usize) -> Target {
        Target {
            path: self.path.clone(),
            line: self.sig[idx].no,
            idx,
        }
    }
}

/// Where each window hash occurs among the changed files, in file then line order.
fn locations(hashes: impl Iterator<Item = (usize, usize, Option<u64>)>) -> HashMap<u64, Vec<(usize, usize)>> {
    let mut map: HashMap<u64, Vec<(usize, usize)>> = HashMap::new();
    for (file, idx, hash) in hashes {
        if let Some(hash) = hash {
            map.entry(hash).or_default().push((file, idx));
        }
    }
    map
}

#[derive(Default)]
struct Hits {
    windows: HashMap<u64, Target>,
    fns: HashMap<u64, Target>,
}

impl Hits {
    fn record(&mut self, ix: &Indexed, windows: &HashSet<u64>, fns: &HashSet<u64>) {
        for (i, hash) in ix.windows.iter().enumerate() {
            if let Some(h) = hash.filter(|h| windows.contains(h)) {
                self.windows.entry(h).or_insert_with(|| ix.target(i));
            }
        }
        for f in &ix.fns {
            if let Some(h) = f.hash.filter(|h| fns.contains(h)) {
                let target = Target {
                    path: ix.path.clone(),
                    line: f.first,
                    idx: 0,
                };
                self.fns.entry(h).or_insert(target);
            }
        }
    }
}

fn scan_corpus(corpus: &dyn Corpus, sources: &[Source], windows: &HashSet<u64>, fns: &HashSet<u64>) -> Hits {
    let mut hits = Hits::default();
    if windows.is_empty() && fns.is_empty() {
        return hits;
    }
    let changed: HashSet<&str> = sources.iter().map(|s| s.path.as_str()).collect();
    let families: HashSet<Lang> = sources.iter().map(|s| s.lang).collect();
    for family in families {
        for (path, text) in corpus
            .files(family)
            .iter()
            .filter(|(p, _)| !changed.contains(p.as_str()))
        {
            hits.record(&Indexed::new(&Source::from_text(path, text)), windows, fns);
        }
    }
    hits
}

fn run_finding(ix: &Indexed, first: usize, last: usize, target: &Target) -> Finding {
    let (start, end) = (ix.sig[first].no, ix.sig[last + WINDOW - 1].no);
    let lines = last - first + WINDOW;
    let message = format!("duplicates {}:{} ({lines} lines)", target.path, target.line);
    Finding::new(&ix.path, start, "duplicate-code", Severity::Warning, &message).spanning(end)
}

struct Run {
    first: usize,
    last: usize,
    target: Target,
    next_idx: usize,
}

fn window_findings(ix: &Indexed, matches: &[Option<Target>]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut run: Option<Run> = None;
    for (i, found) in matches.iter().enumerate() {
        let continues = |r: &Run| {
            found
                .as_ref()
                .is_some_and(|t| t.path == r.target.path && t.idx == r.next_idx)
                && i == r.last + 1
        };
        run = match (run.take(), found) {
            (Some(mut r), Some(t)) if continues(&r) => {
                r.last = i;
                r.next_idx = t.idx + 1;
                Some(r)
            }
            (previous, found) => {
                if let Some(r) = previous {
                    out.push(run_finding(ix, r.first, r.last, &r.target));
                }
                found.as_ref().map(|t| Run {
                    first: i,
                    last: i,
                    target: t.clone(),
                    next_idx: t.idx + 1,
                })
            }
        };
    }
    if let Some(r) = run {
        out.push(run_finding(ix, r.first, r.last, &r.target));
    }
    out
}

fn earlier(locs: &[(usize, usize)], file: usize, idx: usize, gap: usize) -> Option<(usize, usize)> {
    locs.iter()
        .copied()
        .find(|&(f, i)| f < file || (f == file && i + gap <= idx))
}

fn added_window_matches(indexed: &[Indexed]) -> Vec<Vec<Option<Target>>> {
    let all = indexed
        .iter()
        .enumerate()
        .flat_map(|(f, ix)| ix.windows.iter().enumerate().map(move |(i, h)| (f, i, *h)));
    let index = locations(all);
    indexed
        .iter()
        .enumerate()
        .map(|(f, ix)| {
            ix.windows
                .iter()
                .enumerate()
                .map(|(i, hash)| {
                    let locs = index.get(&(*hash).filter(|_| ix.window_is_added(i))?)?;
                    earlier(locs, f, i, WINDOW).map(|(of, oi)| indexed[of].target(oi))
                })
                .collect()
        })
        .collect()
}

fn overlaps(findings: &[Finding], path: &str, first: usize, last: usize) -> bool {
    findings
        .iter()
        .any(|f| f.path == path && f.line <= last && f.end_line.unwrap_or(f.line) >= first)
}

fn function_findings(indexed: &[Indexed], hits: &Hits, covered: &[Finding]) -> Vec<Finding> {
    let all = indexed
        .iter()
        .enumerate()
        .flat_map(|(f, ix)| ix.fns.iter().enumerate().map(move |(i, b)| (f, i, b.hash)));
    let index = locations(all);
    let mut out = Vec::new();
    for (file, ix) in indexed.iter().enumerate() {
        for (i, body) in ix.fns.iter().enumerate().filter(|(_, b)| b.added) {
            let Some(hash) = body.hash else { continue };
            let other = index
                .get(&hash)
                .and_then(|locs| earlier(locs, file, i, 1))
                .map(|(of, oi)| (indexed[of].path.clone(), indexed[of].fns[oi].first))
                .or_else(|| hits.fns.get(&hash).map(|t| (t.path.clone(), t.line)));
            if let Some((path, line)) = other.filter(|_| !overlaps(covered, &ix.path, body.first, body.last)) {
                let message = format!("`{}` has the same body as {path}:{line}", body.name);
                out.push(Finding::new(
                    &ix.path,
                    body.first,
                    "duplicate-function",
                    Severity::Warning,
                    &message,
                ));
            }
        }
    }
    out
}

/// Duplicated code among `sources` (the changed files) and against `corpus`; at most 50 findings.
#[must_use]
pub fn find(sources: &[Source], corpus: &dyn Corpus) -> Vec<Finding> {
    let indexed: Vec<Indexed> = sources.iter().map(Indexed::new).collect();
    let mut matches = added_window_matches(&indexed);
    let mut pending: HashSet<u64> = HashSet::new();
    for (ix, row) in indexed.iter().zip(&matches) {
        let unmatched = row
            .iter()
            .enumerate()
            .filter(|(i, t)| t.is_none() && ix.window_is_added(*i));
        pending.extend(unmatched.filter_map(|(i, _)| ix.windows[i]));
    }
    let pending_fns: HashSet<u64> = indexed
        .iter()
        .flat_map(|ix| ix.fns.iter().filter(|f| f.added).filter_map(|f| f.hash))
        .collect();
    let hits = scan_corpus(corpus, sources, &pending, &pending_fns);
    for (ix, row) in indexed.iter().zip(matches.iter_mut()) {
        for (i, slot) in row.iter_mut().enumerate().filter(|(_, t)| t.is_none()) {
            *slot = ix.windows[i]
                .filter(|_| ix.window_is_added(i))
                .and_then(|h| hits.windows.get(&h).cloned());
        }
    }
    let mut out: Vec<Finding> = indexed
        .iter()
        .zip(&matches)
        .flat_map(|(ix, row)| window_findings(ix, row))
        .collect();
    let functions = function_findings(&indexed, &hits, &out);
    out.extend(functions);
    out.truncate(MAX_FINDINGS);
    out
}

#[cfg(test)]
mod tests;
