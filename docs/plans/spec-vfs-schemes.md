# Specification: Virtual File System Schemes in `read` (`vfs`)

**Status**: Draft  
**Target Milestone**: v0.10.x  
**Affected Crates**: `rho-engine`, `rho-harness-core`

---

## 1. Problem Statement
Agents frequently need context that is not directly a file on the local filesystem:
- Git diffs against branches or commits (`git diff HEAD~1`).
- GitHub Pull Requests, PR diffs, and issue comments.
- Git merge conflicts with conflicting sides (`OURS`, `THEIRS`, `BASE`).

Currently, agents either shell out via `bash` (which requires multi-turn execution and risk of command errors) or rely on `web_fetch` (which has specialized GitHub scraping, but is detached from standard file reading tools like `read`). Tool sprawl (`gh_pr_view`, `git_diff_view`) consumes tool budget tokens and increases cognitive load on the LLM.

## 2. Proposed Solution: Universal Virtual URI Schemes
Extend `read` (and downstream `edit` / `rg`) to transparently resolve virtual URI schemes through a registered VFS resolver pipeline:

1. **`diff://` Scheme**:
   - `read diff://HEAD` -> Working tree changes against HEAD.
   - `read diff://main...feature` -> Branch comparison diff.
   - `read diff://staged` -> Staged git index changes.
2. **`gh://` / `pr://` / `issue://` Scheme**:
   - `read gh://owner/repo/pull/12` or `read pr://12` -> Fetches title, body, discussion, and file list in clean markdown.
   - `read pr://12/diff` -> Unified patch of the pull request.
   - `read issue://45` -> Issue description and comment thread.
3. **`conflict://` Scheme**:
   - `read conflict://` -> Lists all active merge conflict markers across the workspace.
   - `read conflict://path/to/file.rs#1` -> Reads conflict hunk #1 with side-by-side annotations.
   - `write conflict://path/to/file.rs#1` with `@theirs` or `@ours` -> Automatically resolves the conflict hunk in-place.

## 3. Architecture & Interfaces

```rust
pub trait VfsResolver: Send + Sync {
    fn scheme(&self) -> &'static str;
    fn can_resolve(&self, uri: &str) -> bool;
    async fn read(&self, uri: &str, range: Option<LineRange>) -> Result<VfsContent, VfsError>;
}
```

- **Zero Tool Sprawl**: The agent only needs to know `read` and `write`.
- **Consistent Safeguards**: Virtual URIs benefit from the exact same line offset, limit, and truncation guards already implemented in `read`.

## 4. Implementation Steps
1. **Core VFS Dispatcher (`rho-harness-core/src/vfs/`)**:
   - Parse URI schemes (`path.starts_with("scheme://")`).
   - If no scheme, fall through to canonical local workspace filesystem resolver.
2. **Git Schemes**:
   - Implement `diff://` using `gix` or fast in-process git plumbing.
   - Implement `conflict://` parser detecting standard conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`).
3. **GitHub / Remote Schemes**:
   - Reuse existing `web_fetch` GitHub extractor internals for `pr://` and `issue://` when GitHub CLI / token is available.
4. **Tool Wiring**:
   - Teach `read` to invoke `VfsResolver`.
   - Update prompt guidelines and examples in tool descriptions.
