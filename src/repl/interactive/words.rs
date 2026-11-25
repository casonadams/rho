use std::sync::LazyLock;

use super::InteractiveHistory;

const MIN_PREFIX_LEN: usize = 3;
const MIN_WORD_LEN: usize = 4;

const SYSTEM_DICT_PATHS: &[&str] = &[
    "/usr/share/dict/words",
    "/usr/share/dict/american-english",
    "/usr/dict/words",
];

static SYSTEM_DICT: LazyLock<Vec<String>> = LazyLock::new(load_system_dict);

/// Curated common English and software-engineering vocabulary, sorted alphabetically.
const COMMON_WORDS: &[&str] = &[
    "actually",
    "already",
    "always",
    "another",
    "anything",
    "around",
    "awesome",
    "because",
    "before",
    "between",
    "branch",
    "build",
    "change",
    "check",
    "clean",
    "clear",
    "command",
    "commit",
    "compile",
    "complete",
    "completion",
    "config",
    "configuration",
    "confirm",
    "connect",
    "context",
    "continue",
    "control",
    "create",
    "current",
    "custom",
    "debug",
    "default",
    "delete",
    "deploy",
    "describe",
    "design",
    "detail",
    "different",
    "directory",
    "disable",
    "display",
    "document",
    "documentation",
    "download",
    "editor",
    "enable",
    "engine",
    "enough",
    "environment",
    "error",
    "everything",
    "example",
    "execute",
    "existing",
    "expect",
    "explain",
    "export",
    "extension",
    "feature",
    "figure",
    "filter",
    "finish",
    "follow",
    "format",
    "forward",
    "function",
    "generate",
    "github",
    "global",
    "handle",
    "header",
    "helper",
    "history",
    "however",
    "ignore",
    "implement",
    "implementation",
    "import",
    "improve",
    "include",
    "index",
    "inline",
    "input",
    "inspect",
    "install",
    "instead",
    "integration",
    "interface",
    "internal",
    "invalid",
    "issue",
    "keyboard",
    "language",
    "layout",
    "length",
    "level",
    "library",
    "limit",
    "linux",
    "listen",
    "local",
    "logging",
    "machine",
    "macos",
    "markdown",
    "match",
    "memory",
    "merge",
    "message",
    "method",
    "migration",
    "minimal",
    "missing",
    "modal",
    "model",
    "modify",
    "module",
    "monitor",
    "multiple",
    "native",
    "navigate",
    "needed",
    "network",
    "normal",
    "notice",
    "number",
    "object",
    "offset",
    "option",
    "optional",
    "order",
    "original",
    "other",
    "output",
    "overlay",
    "override",
    "package",
    "padding",
    "parallel",
    "parameter",
    "parse",
    "parser",
    "partial",
    "password",
    "patch",
    "pattern",
    "pause",
    "pending",
    "performance",
    "permission",
    "persist",
    "pipeline",
    "place",
    "platform",
    "please",
    "plugin",
    "pointer",
    "policy",
    "position",
    "possible",
    "predict",
    "prediction",
    "prefix",
    "prepare",
    "present",
    "preserve",
    "pretty",
    "preview",
    "previous",
    "print",
    "priority",
    "private",
    "probably",
    "problem",
    "process",
    "profile",
    "project",
    "prompt",
    "property",
    "protocol",
    "provider",
    "public",
    "publish",
    "query",
    "question",
    "queue",
    "quick",
    "quickly",
    "random",
    "range",
    "rather",
    "readme",
    "ready",
    "really",
    "reason",
    "rebuild",
    "recap",
    "recent",
    "record",
    "recover",
    "recovery",
    "redraw",
    "reduce",
    "refactor",
    "reference",
    "refresh",
    "regex",
    "region",
    "register",
    "registry",
    "reject",
    "related",
    "release",
    "reload",
    "remain",
    "remember",
    "remote",
    "remove",
    "rename",
    "render",
    "renderer",
    "repeat",
    "replace",
    "reply",
    "report",
    "repository",
    "request",
    "require",
    "required",
    "reset",
    "resize",
    "resolve",
    "resource",
    "response",
    "restart",
    "restore",
    "result",
    "resume",
    "retry",
    "return",
    "revert",
    "review",
    "reviewer",
    "rewrite",
    "roadmap",
    "route",
    "router",
    "running",
    "runtime",
    "safety",
    "sample",
    "schema",
    "scope",
    "screen",
    "screenshot",
    "script",
    "scroll",
    "search",
    "second",
    "secret",
    "section",
    "security",
    "segment",
    "select",
    "selector",
    "semantic",
    "sender",
    "separate",
    "sequence",
    "server",
    "service",
    "session",
    "setting",
    "settings",
    "setup",
    "several",
    "shadow",
    "share",
    "shell",
    "shift",
    "short",
    "shortcut",
    "should",
    "signal",
    "signature",
    "silent",
    "similar",
    "simple",
    "single",
    "slash",
    "sleep",
    "slice",
    "small",
    "smart",
    "snapshot",
    "socket",
    "something",
    "sometimes",
    "source",
    "space",
    "spawn",
    "special",
    "specific",
    "spinner",
    "split",
    "stack",
    "stage",
    "standard",
    "start",
    "state",
    "static",
    "status",
    "stderr",
    "stdin",
    "stdout",
    "still",
    "storage",
    "store",
    "strategy",
    "stream",
    "streaming",
    "strict",
    "string",
    "strip",
    "strong",
    "struct",
    "structure",
    "structured",
    "style",
    "subagent",
    "submit",
    "subtle",
    "succeed",
    "success",
    "suffix",
    "suggest",
    "suggestion",
    "summary",
    "support",
    "surface",
    "switch",
    "symbol",
    "symlink",
    "syntax",
    "system",
    "table",
    "target",
    "telemetry",
    "template",
    "temporary",
    "terminal",
    "testing",
    "theme",
    "there",
    "these",
    "thing",
    "think",
    "thinking",
    "third",
    "those",
    "though",
    "thought",
    "thread",
    "threshold",
    "through",
    "timeout",
    "timer",
    "title",
    "today",
    "together",
    "toggle",
    "token",
    "tokens",
    "tomorrow",
    "toolbar",
    "tooltip",
    "total",
    "touch",
    "trace",
    "track",
    "tracker",
    "trailing",
    "transcript",
    "transfer",
    "transform",
    "transition",
    "transparent",
    "transport",
    "trigger",
    "trimmed",
    "truncate",
    "trying",
    "tunnel",
    "typing",
    "unable",
    "under",
    "understand",
    "understood",
    "unicode",
    "uniform",
    "unique",
    "unknown",
    "unless",
    "unlock",
    "until",
    "untracked",
    "update",
    "upgrade",
    "upload",
    "upstream",
    "usage",
    "useful",
    "using",
    "usually",
    "utility",
    "valid",
    "validate",
    "validation",
    "value",
    "variable",
    "variant",
    "vector",
    "verbose",
    "verify",
    "version",
    "vertical",
    "viewer",
    "virtual",
    "visible",
    "vision",
    "visual",
    "waiting",
    "warning",
    "watch",
    "watcher",
    "website",
    "weight",
    "welcome",
    "whatever",
    "where",
    "whether",
    "which",
    "while",
    "whitespace",
    "whole",
    "widget",
    "width",
    "window",
    "windows",
    "wired",
    "wiring",
    "within",
    "without",
    "worker",
    "workflow",
    "working",
    "workspace",
    "worktree",
    "world",
    "would",
    "wrapper",
    "write",
    "writer",
    "writing",
    "written",
    "wrong",
    "yellow",
    "yesterday",
    "yourself",
];

/// Instant cross-platform local word completer with three priority tiers:
/// recent history words, curated common vocabulary, and optional system dictionary.
pub struct WordCompleter;

impl WordCompleter {
    /// Returns the completion suffix for the trailing word in `draft`, or `None`
    /// if the trailing token is ineligible or no longer word matches.
    pub fn complete_draft(draft: &str, history: &InteractiveHistory) -> Option<String> {
        let token = extract_trailing_word(draft)?;
        let prefix_lower = token.to_ascii_lowercase();
        let word = Self::complete_prefix(&prefix_lower, history)?;
        let suffix = &word[prefix_lower.len()..];
        let all_caps = token.bytes().all(|b| b.is_ascii_uppercase());
        if all_caps {
            Some(suffix.to_ascii_uppercase())
        } else {
            Some(suffix.to_string())
        }
    }

    fn complete_prefix(prefix_lower: &str, history: &InteractiveHistory) -> Option<String> {
        if is_exact_known_word(prefix_lower, history) {
            return None;
        }
        find_in_history(prefix_lower, history)
            .or_else(|| find_in_sorted_slice(prefix_lower, COMMON_WORDS).map(str::to_string))
            .or_else(|| find_in_sorted_strings(prefix_lower, &SYSTEM_DICT).map(str::to_string))
    }
}

fn is_exact_known_word(prefix_lower: &str, history: &InteractiveHistory) -> bool {
    if prefix_lower.len() < MIN_WORD_LEN {
        return false;
    }
    COMMON_WORDS.binary_search(&prefix_lower).is_ok()
        || SYSTEM_DICT.binary_search_by(|w| w.as_str().cmp(prefix_lower)).is_ok()
        || history
            .entries()
            .iter()
            .flat_map(|entry| entry.split(|c: char| !c.is_ascii_alphabetic()))
            .any(|w| w.eq_ignore_ascii_case(prefix_lower))
}

fn extract_trailing_word(draft: &str) -> Option<&str> {
    if draft.starts_with('/') {
        return None;
    }
    let bytes = draft.as_bytes();
    let mut start = bytes.len();
    while start > 0 && bytes[start - 1].is_ascii_alphabetic() {
        start -= 1;
    }
    let token = &draft[start..];
    if token.len() < MIN_PREFIX_LEN {
        return None;
    }
    if start > 0 && matches!(bytes[start - 1], b'/' | b'\\' | b'.' | b'@' | b':') {
        return None;
    }
    Some(token)
}

fn find_in_history(prefix_lower: &str, history: &InteractiveHistory) -> Option<String> {
    history
        .entries()
        .iter()
        .rev()
        .flat_map(|entry| entry.split(|c: char| !c.is_ascii_alphabetic()))
        .filter(|w| {
            w.len() >= MIN_WORD_LEN
                && w.len() > prefix_lower.len()
                && w.as_bytes()[..prefix_lower.len()].eq_ignore_ascii_case(prefix_lower.as_bytes())
        })
        .min_by_key(|w| w.len())
        .map(str::to_ascii_lowercase)
}

fn find_in_sorted_slice<'a>(prefix_lower: &str, words: &[&'a str]) -> Option<&'a str> {
    let start = words.partition_point(|&w| w < prefix_lower);
    words[start..]
        .iter()
        .copied()
        .take_while(|w| w.starts_with(prefix_lower))
        .filter(|w| w.len() > prefix_lower.len())
        .min_by_key(|w| w.len())
}

fn find_in_sorted_strings<'a>(prefix_lower: &str, words: &'a [String]) -> Option<&'a str> {
    let start = words.partition_point(|w| w.as_str() < prefix_lower);
    words[start..]
        .iter()
        .map(String::as_str)
        .take_while(|w| w.starts_with(prefix_lower))
        .filter(|w| w.len() > prefix_lower.len())
        .min_by_key(|w| w.len())
}

fn load_system_dict() -> Vec<String> {
    let Some(contents) = SYSTEM_DICT_PATHS
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
    else {
        return Vec::new();
    };
    let mut words: Vec<String> = contents
        .lines()
        .map(str::trim)
        .filter(|w| w.len() >= MIN_WORD_LEN && w.bytes().all(|b| b.is_ascii_alphabetic()))
        .map(str::to_ascii_lowercase)
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_history() -> (InteractiveHistory, rho_test_support::TestDir) {
        let dir = rho_test_support::TestDir::new();
        let history = InteractiveHistory::with_file(20, dir.join("history.txt")).unwrap();
        (history, dir)
    }

    #[test]
    fn common_words_are_sorted_unique_and_valid() {
        for pair in COMMON_WORDS.windows(2) {
            assert!(
                pair[0] < pair[1],
                "out of order or duplicate: {:?} >= {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(
            COMMON_WORDS
                .iter()
                .all(|w| w.len() >= MIN_WORD_LEN && w.bytes().all(|b| b.is_ascii_lowercase()))
        );
    }

    #[test]
    fn completes_common_word_and_preserves_all_caps() {
        let (history, _dir) = empty_history();
        assert_eq!(
            WordCompleter::complete_draft("that's awe", &history),
            Some("some".to_string())
        );
        assert_eq!(WordCompleter::complete_draft("AWE", &history), Some("SOME".to_string()));
        assert_eq!(WordCompleter::complete_draft("Awe", &history), Some("some".to_string()));
    }

    #[test]
    fn history_word_beats_common_and_dictionary_words() {
        let (mut history, _dir) = empty_history();
        history.record("explore ripwire architecture").unwrap();
        assert_eq!(
            WordCompleter::complete_draft("check rip", &history),
            Some("wire".to_string())
        );

        // History word also overrides COMMON_WORDS when it matches.
        history.record("use awesomeness").unwrap();
        assert_eq!(
            WordCompleter::complete_draft("awe", &history),
            Some("someness".to_string())
        );
    }

    #[test]
    fn skips_ineligible_tokens() {
        let (history, _dir) = empty_history();
        assert_eq!(WordCompleter::complete_draft("src/awe", &history), None);
        assert_eq!(WordCompleter::complete_draft("C:\\awe", &history), None);
        assert_eq!(WordCompleter::complete_draft("foo.awe", &history), None);
        assert_eq!(WordCompleter::complete_draft("@awe", &history), None);
        assert_eq!(WordCompleter::complete_draft("http:awe", &history), None);
        assert_eq!(WordCompleter::complete_draft("/awe", &history), None);
        assert_eq!(WordCompleter::complete_draft("aw", &history), None);
        assert_eq!(WordCompleter::complete_draft("", &history), None);
    }

    #[test]
    fn suffix_is_always_single_line() {
        let (mut history, _dir) = empty_history();
        history.record("multi\nline\rwordcompletion").unwrap();
        let suffix = WordCompleter::complete_draft("wordcomp", &history);
        assert_eq!(suffix, Some("letion".to_string()));
        assert!(suffix.as_deref().is_some_and(|s| !s.contains(['\n', '\r'])));
    }
}
