//! The documentation the site shows: the repository's README and `docs/*.md`, compiled in, so
//! the site always matches the Markdown it is built from.

use crate::route::Route;

const REPO_BLOB: &str = "https://github.com/casonadams/rho/blob/main/";
const REPO_TREE: &str = "https://github.com/casonadams/rho/tree/main/";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Doc {
    Overview,
    Configuration,
    Ui,
    Shortcuts,
    Workflows,
    Permissions,
    McpAndHooks,
}

impl Doc {
    pub const ALL: [Doc; 7] = [
        Doc::Overview,
        Doc::Configuration,
        Doc::Permissions,
        Doc::McpAndHooks,
        Doc::Workflows,
        Doc::Ui,
        Doc::Shortcuts,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Doc::Overview => "overview",
            Doc::Configuration => "configuration",
            Doc::Ui => "ui",
            Doc::Shortcuts => "shortcuts",
            Doc::Workflows => "workflows",
            Doc::Permissions => "permissions",
            Doc::McpAndHooks => "mcp-and-hooks",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Doc::Overview => "Overview",
            Doc::Configuration => "Providers & configuration",
            Doc::Ui => "Interface & styling",
            Doc::Shortcuts => "Keyboard shortcuts",
            Doc::Workflows => "Workflows",
            Doc::Permissions => "Permissions & privacy",
            Doc::McpAndHooks => "MCP & hooks",
        }
    }

    /// The file's path in the repository.
    pub fn source_path(self) -> &'static str {
        match self {
            Doc::Overview => "README.md",
            Doc::Configuration => "docs/configuration.md",
            Doc::Ui => "docs/ui.md",
            Doc::Shortcuts => "docs/shortcuts.md",
            Doc::Workflows => "docs/workflows.md",
            Doc::Permissions => "docs/permissions.md",
            Doc::McpAndHooks => "docs/mcp-and-hooks.md",
        }
    }

    pub fn markdown(self) -> &'static str {
        match self {
            Doc::Overview => include_str!("../../README.md"),
            Doc::Configuration => include_str!("../../docs/configuration.md"),
            Doc::Ui => include_str!("../../docs/ui.md"),
            Doc::Shortcuts => include_str!("../../docs/shortcuts.md"),
            Doc::Workflows => include_str!("../../docs/workflows.md"),
            Doc::Permissions => include_str!("../../docs/permissions.md"),
            Doc::McpAndHooks => include_str!("../../docs/mcp-and-hooks.md"),
        }
    }

    pub fn from_slug(slug: &str) -> Option<Doc> {
        Doc::ALL.into_iter().find(|doc| doc.slug() == slug)
    }

    fn from_source_path(path: &str) -> Option<Doc> {
        Doc::ALL.into_iter().find(|doc| doc.source_path() == path)
    }

    fn source_dir(self) -> &'static str {
        match self {
            Doc::Overview => "",
            _ => "docs/",
        }
    }
}

/// A run of a document from one heading to the next, shown as one block that a link to its
/// heading scrolls to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// The heading's text, for the page's outline; empty for text before the first heading.
    pub title: String,
    /// The heading's level, 1 to 6; 0 for text before the first heading.
    pub level: usize,
    /// The heading's anchor, GitHub's way.
    pub anchor: Option<String>,
    pub markdown: &'static str,
}

/// `markdown` split before each heading outside fenced code.
pub fn sections(markdown: &'static str) -> Vec<Section> {
    let mut starts = vec![0];
    let mut fence: Option<&str> = None;
    let mut offset = 0;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        match fence {
            Some(open) if trimmed.starts_with(open) => fence = None,
            Some(_) => {}
            None if trimmed.starts_with("```") || trimmed.starts_with("~~~") => {
                fence = Some(&trimmed[..3]);
            }
            None if offset > 0 && heading_level(line).is_some() => starts.push(offset),
            None => {}
        }
        offset += line.len();
    }
    starts.push(markdown.len());
    starts
        .windows(2)
        .map(|w| section(&markdown[w[0]..w[1]]))
        .filter(|s| !s.markdown.trim().is_empty())
        .collect()
}

fn section(markdown: &'static str) -> Section {
    let first = markdown.lines().next().unwrap_or_default();
    match heading_level(first) {
        Some(level) => {
            let heading = first[level..].trim();
            Section {
                title: plain(heading),
                level,
                anchor: Some(slug(heading)),
                markdown,
            }
        }
        None => Section {
            title: String::new(),
            level: 0,
            anchor: None,
            markdown,
        },
    }
}

fn heading_level(line: &str) -> Option<usize> {
    let level = line.bytes().take_while(|b| *b == b'#').count();
    let rest = &line[level..];
    ((1..=6).contains(&level) && rest.starts_with(' ')).then_some(level)
}

/// Heading text without Markdown code and emphasis marks.
fn plain(text: &str) -> String {
    text.chars().filter(|c| !matches!(c, '`' | '*' | '_')).collect()
}

/// A heading's anchor as GitHub makes it: lowercase, punctuation dropped, spaces as hyphens.
pub fn slug(heading: &str) -> String {
    heading
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// A built-in tool as the README's "Built-in Tools" list describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    /// The first sentence of its description.
    pub summary: String,
}

/// The tools the README lists under "Built-in Tools", in its order.
pub fn tools() -> Vec<Tool> {
    let readme = Doc::Overview.markdown();
    let Some(start) = readme.find("## Built-in Tools") else {
        return Vec::new();
    };
    let list = &readme[start..];
    let list = &list[..list[3..].find("\n## ").map_or(list.len(), |end| end + 3)];
    let mut tools: Vec<(String, String)> = Vec::new();
    for line in list.lines() {
        if let Some(rest) = line.strip_prefix("- **`") {
            let (name, description) = rest.split_once("`**").unwrap_or((rest, ""));
            let description = description.trim_start_matches(':').trim();
            tools.push((name.to_owned(), description.to_owned()));
        } else if let (Some(last), true) = (tools.last_mut(), line.starts_with("  ")) {
            last.1.push(' ');
            last.1.push_str(line.trim());
        }
    }
    tools
        .into_iter()
        .map(|(name, description)| Tool {
            summary: first_sentence(&strip_inline(&description)),
            name,
        })
        .collect()
}

fn first_sentence(text: &str) -> String {
    match text.find(". ") {
        Some(end) => text[..=end].to_owned(),
        None => text.trim().to_owned(),
    }
}

/// Text without Markdown code marks, emphasis and link targets.
fn strip_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("](") {
        out.push_str(&rest[..open]);
        rest = rest[open + 2..].split_once(')').map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out.chars().filter(|c| !matches!(c, '`' | '*' | '[')).collect()
}

/// Where a link in a document goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// A page of the site, at a heading when the link names one.
    Page(Route, Option<String>),
    /// A heading of the document the link is in.
    Anchor(String),
    External(String),
}

pub fn resolve_link(from: Doc, href: &str) -> LinkTarget {
    if href.contains("://") || href.starts_with("mailto:") {
        return LinkTarget::External(href.to_owned());
    }
    let (path, anchor) = match href.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor.to_owned())),
        None => (href, None),
    };
    if path.is_empty() {
        return LinkTarget::Anchor(anchor.unwrap_or_default());
    }
    let repo_path = normalize(&format!("{}{path}", from.source_dir()));
    if let Some(doc) = Doc::from_source_path(&repo_path) {
        return LinkTarget::Page(Route::Docs(doc), anchor);
    }
    if repo_path == "docs" || repo_path.is_empty() {
        return LinkTarget::Page(Route::Docs(Doc::Overview), anchor);
    }
    let base = if href.ends_with('/') { REPO_TREE } else { REPO_BLOB };
    LinkTarget::External(format!("{base}{repo_path}"))
}

/// A repository path with `.` and `..` segments applied and no trailing slash.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_list_comes_from_the_readme() {
        let tools = tools();
        assert_eq!(tools.len(), 16);
        assert!(Doc::Overview.markdown().contains("includes 16 fast, native tools"));
        let read = &tools[0];
        assert_eq!(read.name, "read");
        assert_eq!(
            read.summary,
            "Read file contents with line numbering, offset, and limit safeguards."
        );
        let subagent = tools.iter().find(|t| t.name == "subagent").unwrap();
        assert!(subagent.summary.contains("custom agent"), "{}", subagent.summary);
        assert!(!subagent.summary.contains("]("));
    }

    #[test]
    fn slugs_follow_github() {
        let cases = [
            ("Subagents & Custom Agents", "subagents--custom-agents"),
            ("Memory Notes", "memory-notes"),
            (
                "Permission Rules (`permission.toml`)",
                "permission-rules-permissiontoml",
            ),
            ("Guidance on `qwen2.5-coder:7b`", "guidance-on-qwen25-coder7b"),
        ];
        for (heading, want) in cases {
            assert_eq!(slug(heading), want, "{heading}");
        }
    }

    #[test]
    fn sections_split_at_every_heading_outside_code() {
        let doc = "# Title\nintro\n\n## One\ntext\n```sh\n## not a heading\n```\n### Deep\n## Two\nend\n";
        let got = sections(doc);
        let heads: Vec<(&str, usize)> = got.iter().map(|s| (s.title.as_str(), s.level)).collect();
        assert_eq!(heads, [("Title", 1), ("One", 2), ("Deep", 3), ("Two", 2)]);
        assert_eq!(got[2].anchor.as_deref(), Some("deep"));
        assert!(got[1].markdown.contains("## not a heading"));
    }

    #[test]
    fn every_doc_has_titled_sections() {
        for doc in Doc::ALL {
            let sections = sections(doc.markdown());
            assert!(sections.len() > 1, "{doc:?}");
            assert!(sections.iter().skip(1).all(|s| !s.title.is_empty()), "{doc:?}");
        }
    }

    #[test]
    fn links_resolve_to_pages_anchors_or_the_repository() {
        let page = |doc, anchor: Option<&str>| LinkTarget::Page(Route::Docs(doc), anchor.map(str::to_owned));
        let cases = [
            (
                Doc::Overview,
                "docs/configuration.md#personas",
                page(Doc::Configuration, Some("personas")),
            ),
            (Doc::Overview, "docs/", page(Doc::Overview, None)),
            (Doc::Workflows, "configuration.md", page(Doc::Configuration, None)),
            (Doc::Workflows, "#worktrees", LinkTarget::Anchor("worktrees".into())),
            (
                Doc::McpAndHooks,
                "../examples/hooks/guard-bash.sh",
                LinkTarget::External(format!("{REPO_BLOB}examples/hooks/guard-bash.sh")),
            ),
            (
                Doc::Workflows,
                "../.agents/workflows/refine/",
                LinkTarget::External(format!("{REPO_TREE}.agents/workflows/refine")),
            ),
            (
                Doc::Ui,
                "https://example.com/x",
                LinkTarget::External("https://example.com/x".into()),
            ),
        ];
        for (from, href, want) in cases {
            assert_eq!(resolve_link(from, href), want, "{href}");
        }
    }

    #[test]
    fn every_relative_link_in_the_docs_resolves_to_a_known_target() {
        for doc in Doc::ALL {
            let doc_sections = sections(doc.markdown());
            let anchors: Vec<&String> = doc_sections.iter().filter_map(|s| s.anchor.as_ref()).collect();
            for href in links(doc.markdown()) {
                match resolve_link(doc, href) {
                    LinkTarget::Anchor(anchor) => {
                        assert!(anchors.contains(&&anchor), "{doc:?}: #{anchor}");
                    }
                    LinkTarget::Page(Route::Docs(target), Some(anchor)) => {
                        let found = sections(target.markdown())
                            .iter()
                            .any(|s| s.anchor.as_ref() == Some(&anchor));
                        assert!(found, "{doc:?}: {href}");
                    }
                    _ => {}
                }
            }
        }
    }

    fn links(markdown: &str) -> impl Iterator<Item = &str> {
        markdown.split("](").skip(1).filter_map(|rest| rest.split(')').next())
    }
}
