# Specialized Web Fetch Extractors (GitHub & YouTube) Spec

## Status

Draft

## Problem

When the agent uses the `web_fetch` tool to retrieve content from `github.com` or `youtube.com`:
1. **GitHub Pages:** Standard HTML scraping retrieves bloated, heavily script-dependent DOM trees with hundreds of lines of navigation, sidebars, cookie banners, reaction buttons, and UI chrome, while the actual issue discussion, PR diff, commit change list, or raw code is buried or missing entirely.
2. **YouTube Video Pages:** YouTube relies on client-side SPA rendering with dynamic hydration. Standard HTML scraping either returns empty content, triggers SPA shell failure (`"Page contains little static content and may require a JavaScript-capable browser"`), or delivers useless script tag blobs instead of the video metadata and timed dialogue/captions that the LLM needs for comprehension.

Issue #38 requests specialized fetch extractors for `github.com` and `youtube.com` so that `web_fetch` delivers high-density, clean Markdown representations of GitHub resources (issues, PRs, commits, blobs, trees) and YouTube videos (metadata and timed transcript dialogue).

## Users and Stakeholders

- **Agent / LLM Tool Calling:** Needs context-dense, token-efficient Markdown without HTML noise when inspecting GitHub repositories, code, pull requests, issues, or YouTube video transcripts.
- **Developer / User:** Investigating external libraries, reading bug reports, checking PR diffs, or asking questions about video tutorials without having to manually copy-paste transcripts or diffs.
- **API Consumers:** Expects existing `web_fetch` API contracts (`url`, `offset`, `limit`, `mode`, `format`) and caching behavior to continue functioning predictably.

## Goals

- **GitHub Interception & Extraction:**
  - Intercept `github.com` URLs targeting issues (`/issues/{id}`), pull requests (`/pull/{id}`), commits (`/commit/{sha}`), file blobs (`/blob/{ref}/{path}`), and directory trees (`/tree/{ref}/{path}`).
  - Authenticate against GitHub REST API using `GITHUB_TOKEN` or `GH_TOKEN` environment variables when present, with graceful unauthenticated fallback (subject to GitHub rate limits).
  - Extract and format issues: title, number, author, status, labels, description body, and discussion comments.
  - Extract and format pull requests: title, number, author, status, base/head branches, changes summary, description, comments, and unified diff.
  - Extract and format commits: commit subject, author, commit date, stats (+/- counts), parent SHAs, commit message body, and per-file unified diffs.
  - Extract file blobs directly as raw file content without HTML wrappers (via `raw.githubusercontent.com` or raw endpoints).
  - Extract directory trees as structured file listings.
- **YouTube Interception & Extraction:**
  - Intercept video URLs for `youtube.com/watch?v=...`, `youtu.be/...`, `youtube.com/shorts/...`, and `youtube.com/embed/...`.
  - Extract video metadata: Title, Channel/Author, Duration, Upload date, Views, and Description.
  - Extract closed captions / timed-text transcripts directly from YouTube's player response / timedtext service.
  - Format dialogue chronologically with timestamps (`[MM:SS]` or `[HH:MM:SS]`) into clean, readable text suitable for LLM context.
  - Fallback gracefully when a video has no closed captions or disabled transcripts by returning complete video metadata and an informative notice instead of failing with an SPA shell error.
- **Architecture & Resilience:**
  - Maintain `web_fetch` pagination (`offset`, `limit`) and result caching in `FetchCache`.
  - Provide an escape hatch: if the user specifies `format = "html"`, bypass specialized extraction and perform generic HTML scraping.
  - Zero required external binaries: perform all extraction in pure Rust using existing workspace dependencies (`reqwest`, `serde`, `serde_json`, `quick-xml`, `regex`, `url`).

## Non-goals

- Downloading binary media (audio/video files) or generating video summaries via external transcription models (e.g. Whisper).
- Requiring `yt-dlp` or Python runtime to extract YouTube transcripts; the primary extractor must be native HTTP/XML/JSON.
- Writing to GitHub (submitting comments, creating PRs) via `web_fetch`; this is strictly read-only content retrieval.
- Supporting private GitHub repositories without a valid token; private resources with missing/invalid tokens will return standard HTTP 404/401 errors.

## Current Behavior

- In `crates/rho-engine/src/tools/web/fetch/mod.rs`:
  - `fetch_and_extract` issues a generic `self.http.get_bytes(...)` request to the target URL.
  - If the response is not a PDF, it decodes the response body as text and passes it to `extract::extract_text(...)`.
- In `crates/rho-engine/src/tools/web/fetch/extract/html.rs`:
  - Uses `scraper` to parse the HTML document.
  - Evaluates `check_spa_shell`: if text length is small and script tags > 500 chars or `#root`/`#app` exists, returns `AppError::Tool("Page contains little static content and may require a JavaScript-capable browser")`.
  - GitHub issue/PR pages yield noisy HTML with sidebar navigation and minimal comment structure.
  - YouTube pages fail immediately under `check_spa_shell` or produce unhelpful boilerplate.

## Desired Behavior

- In `crates/rho-engine/src/tools/web/fetch/mod.rs`:
  - Before performing generic HTTP fetch, `fetch_and_extract` inspects the target URL.
  - If `options.format_override == Some("html")`, skip interceptors and use generic fetch/extract.
  - If the URL matches a supported GitHub resource:
    - Route to `extract::github::extract_github(...)`.
  - If the URL matches a YouTube video:
    - Route to `extract::youtube::extract_youtube(...)`.
  - Otherwise, continue with existing generic fetch and extract pipeline.
- GitHub Extractor output:
  - Formats issues with header metadata (status, author, date, labels), body, and numbered comments with author and date.
  - Formats pull requests with header metadata, description, comments, and diff section.
  - Formats commits with SHA, author, stats, commit message, and diff blocks.
  - Formats blobs with file path and raw text content.
  - Formats trees with directory hierarchy and file entries.
- YouTube Extractor output:
  - Header with `# <Title>`, Channel, Duration, Uploaded date, Views, Video ID.
  - Collapsible or delimited Description section.
  - `## Transcript (<language>)` section with timestamped lines (e.g., `[00:15] Hello everyone and welcome...`).
  - If captions are unavailable, displays `*No transcript available for this video.*` while preserving all metadata.

## Requirements

- **REQ-001 (URL Recognition):** Parse and classify URLs for GitHub and YouTube.
  - GitHub: Detect `github.com` and classify into `Issue`, `PullRequest`, `Commit`, `Blob`, `Tree`, or `Other`.
  - YouTube: Detect `youtube.com` (and `m.youtube.com`, `youtu.be`) and extract the 11-character video ID from `/watch?v=`, `/embed/`, `/v/`, `/shorts/`, or path on `youtu.be`.
- **REQ-002 (GitHub Authentication):**
  - Read `GITHUB_TOKEN` and `GH_TOKEN` environment variables.
  - When available, attach `Authorization: Bearer <token>` to GitHub API requests.
  - Always send a descriptive `User-Agent: rho-agent` header on GitHub API calls to comply with GitHub API policies.
  - Handle rate limits (HTTP 403 with `x-ratelimit-remaining: 0`): return a clear error stating the GitHub rate limit has been exceeded and advising setting `GITHUB_TOKEN`.
- **REQ-003 (GitHub Issues & Pull Requests):**
  - Retrieve issue/PR metadata via `https://api.github.com/repos/{owner}/{repo}/issues/{number}` (or `/pulls/{number}`).
  - Retrieve comments via `https://api.github.com/repos/{owner}/{repo}/issues/{number}/comments`.
  - For PRs, retrieve diff via `.diff` endpoint or `application/vnd.github.v3.diff` Accept header.
  - Render as clean Markdown without HTML tags, UI navigation, or reaction counters.
- **REQ-004 (GitHub Commits & Trees):**
  - Commits: Query `/repos/{owner}/{repo}/commits/{sha}`; format message, stats, and `files[].patch` diffs.
  - Blobs: Convert to `raw.githubusercontent.com/{owner}/{repo}/{ref}/{path}` (or fetch raw via API) and return content.
  - Trees: Query `/repos/{owner}/{repo}/contents/{path}?ref={ref}` and format directory listing.
- **REQ-005 (YouTube Metadata & Player Response):**
  - Fetch watch HTML (`https://www.youtube.com/watch?v={video_id}`) with standard desktop user agent and language headers (`Accept-Language: en-US,en;q=0.9`).
  - Extract `ytInitialPlayerResponse` JSON embedded in script tags.
  - Extract `videoDetails`: `title`, `author` (channel), `lengthSeconds`, `viewCount`, `shortDescription`.
- **REQ-006 (YouTube Timed-Text Transcript):**
  - Locate `captions.playerCaptionsTracklistRenderer.captionTracks` in `ytInitialPlayerResponse`.
  - Select English caption track (`en`, `en-US`, etc.) or the first available track.
  - Fetch timed-text XML/JSON from track `baseUrl`.
  - Parse cue timestamps and text using `quick-xml` or JSON parser, decoding XML entities (e.g. `&amp;`, `&#39;`).
  - Deduplicate consecutive repeated auto-generated cues and group into chronological timestamped lines `[MM:SS] text`.
- **REQ-007 (Format Override & Fallback):**
  - If `WebFetchArgs.format == Some("html")`, bypass specialized extractors and run standard HTML fetch.
  - If an intercepted extractor fails due to non-fatal extraction issues (e.g. unrecognized GitHub sub-URL), fall back cleanly to standard HTML fetch.
- **REQ-008 (Performance & Safety):**
  - Respect `timeout_sec` and `max_bytes` configurations from `WebFetchTool`.
  - Use `FetchCache` to avoid re-fetching identical URLs with same options.
  - Limit comment and commit pagination to avoid unbounded memory growth (e.g. max 100 comments, max 50 changed files).

## Invariants and Security Boundaries

- **Credential Safety:** Never expose `GITHUB_TOKEN` in returned Markdown, error messages, or cache keys.
- **Private Network Protection:** Adhere to `assert_public_dns` and private network protection in `HttpClient`.
- **Domain vs Transport Separation:** Extraction and markdown formatting logic must remain pure, unit-testable functions operating on data structures and responses, independent of TUI or REPL transports.
- **Zero Subprocess Requirement:** Default YouTube and GitHub extraction must not spawn external processes (`yt-dlp`, `curl`, `python`).

## Definition of Done

- Unit tests cover:
  - GitHub URL parsing across all variants (`issue`, `pull`, `commit`, `blob`, `tree`, `shorts`, `youtu.be`).
  - GitHub Issue and PR response rendering into clean Markdown.
  - GitHub Commit response rendering with unified diffs.
  - YouTube player response metadata parsing.
  - YouTube timed-text XML parsing, entity unescaping, deduplication, and timestamp formatting.
  - Fallback when no captions are available.
  - Format override behavior (`format = "html"`).
- End-to-end tool tests prove `WebFetchTool` properly intercepts and formats responses.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `make crap` all pass with no warnings or regressions.
- Documentation in `docs/` and `www/` updated to reflect specialized fetch capabilities.

## Acceptance Criteria

- AC-001: Given a GitHub issue URL (`https://github.com/owner/repo/issues/1`), when `web_fetch` is called, it returns Markdown containing the title, issue state, author, description, and comments, without HTML tags or UI chrome.
- AC-002: Given a GitHub commit URL (`https://github.com/owner/repo/commit/<sha>`), when `web_fetch` is called, it returns Markdown containing the commit message, author, stats, and ````diff``` blocks of file changes.
- AC-003: Given a GitHub blob URL (`https://github.com/owner/repo/blob/main/Cargo.toml`), when `web_fetch` is called, it returns the raw file content.
- AC-004: Given a YouTube watch URL (`https://www.youtube.com/watch?v=dQw4w9WgXcQ`), when `web_fetch` is called, it returns the video title, channel, duration, and timestamped transcript dialogue.
- AC-005: Given a YouTube short URL (`https://youtu.be/<id>` or `https://youtube.com/shorts/<id>`), when `web_fetch` is called, it correctly resolves the video ID and extracts metadata and transcript.
- AC-006: Given a YouTube video without captions, when `web_fetch` is called, it returns video metadata and `*No transcript available for this video.*` without throwing an SPA shell error.
- AC-007: Given `format: "html"`, when `web_fetch` is called on a GitHub or YouTube URL, specialized extractors are bypassed and raw HTML scraping is performed.
- AC-008: Given an environment with `GITHUB_TOKEN` set, GitHub API calls include the `Authorization` header.

## Edge Cases

- **GitHub rate limiting:** Unauthenticated requests receive 403 when hourly quota is exhausted. Extractor must catch 403 rate limit responses and return a clear, actionable error message.
- **Empty or single-cue captions:** Videos with 0 or 1 caption cues must not crash the parser.
- **Auto-generated caption duplicates:** YouTube auto-generated captions stream word-by-word with overlapping cues; the cleaner must collapse duplicates and format readable lines.
- **HTML entities in captions:** Captions containing `&amp;`, `&#39;`, `&quot;`, `&gt;`, `&lt;` must be decoded to plain characters.
- **Large commit diffs:** Commits with hundreds of files or binary files must truncate diffs and note binary/large files cleanly.
- **Pagination of issue comments:** Issues with >30 comments fetch additional comments up to a safety threshold (100 comments).

## Constraints

- No new external crate dependencies. Use existing workspace dependencies (`quick-xml`, `scraper`, `reqwest`, `serde`, `serde_json`, `url`, `regex`).
- All code must satisfy the CRAP threshold (`make crap` <= 30 per function).
- Functions must remain small and focused (cyclomatic complexity <= 5 where possible, parameter counts <= 6).

## Risks and Mitigations

- **Risk:** YouTube changes its internal `ytInitialPlayerResponse` JSON structure or caption track format.
  - **Mitigation:** Robust fallback parsing with multiple selector strategies (regex extraction on script tags, JSON pointer lookup) and graceful degradation to metadata-only display if caption tracks are absent.
- **Risk:** GitHub API unauthenticated rate limit (60 requests/hour) exhausted during intensive browsing.
  - **Mitigation:** Cache responses in `FetchCache`, prefer raw endpoints for blobs and diffs where possible, and provide explicit user guidance on configuring `GITHUB_TOKEN`.

## References

- Issue #38: `https://github.com/casonadams/rho/issues/38`
- Reference implementation in `../oh-my-pi`:
  - `packages/coding-agent/src/web/scrapers/github.ts`
  - `packages/coding-agent/src/web/scrapers/youtube.ts`
- YouTube timedtext API formats (XML `<transcript><text start="..." dur="...">` and JSON3)
- GitHub REST API documentation (`/repos/{owner}/{repo}/issues`, `/pulls`, `/commits`, `/contents`)
