# Implementation Plan: Specialized Fetch Extractors for GitHub and YouTube

## Goal

Add specialized URL interceptors and extractors to `rho`'s `web_fetch` tool for GitHub (`github.com`) and YouTube (`youtube.com`, `youtu.be`) to transform issues, PR diffs, commits, raw blobs, directory trees, and YouTube video transcripts into clean, token-efficient Markdown without noisy UI boilerplate or SPA rendering failures.

## Reference Spec

`.specs/fetch-extractors/spec.md`

## 1. Research

- **GitHub REST API & Endpoints:**
  - Issue metadata & comments: `GET https://api.github.com/repos/{owner}/{repo}/issues/{number}` and `.../issues/{number}/comments`.
  - Pull request details & diffs: `GET https://api.github.com/repos/{owner}/{repo}/pulls/{number}` and `https://github.com/{owner}/{repo}/pull/{number}.diff` (or `Accept: application/vnd.github.v3.diff`).
  - Commits: `GET https://api.github.com/repos/{owner}/{repo}/commits/{sha}` returns `stats` and unified diff patches for changed files (`files[].patch`).
  - Raw file blobs: `https://raw.githubusercontent.com/{owner}/{repo}/{ref}/{path}` serves raw file text directly without markup.
  - Directory contents: `GET https://api.github.com/repos/{owner}/{repo}/contents/{path}?ref={ref}` returns structured JSON of directory children.
  - Authentication: GitHub requires a `User-Agent` header. Requests with `Authorization: Bearer <token>` get 5,000 req/hr; unauthenticated requests get 60 req/hr.
- **YouTube Timed-Text & Player Response:**
  - Watch pages (`https://www.youtube.com/watch?v={id}`) embed `ytInitialPlayerResponse = {...};` in an inline `<script>` tag.
  - Video metadata is in `videoDetails` (`title`, `author`, `lengthSeconds`, `viewCount`, `shortDescription`).
  - Caption track descriptors are located at `captions.playerCaptionsTracklistRenderer.captionTracks` with attributes `baseUrl`, `languageCode`, `name.simpleText`, and `kind` (e.g. `asr` for automatic speech recognition).
  - Querying `baseUrl` returns XML timed-text with elements `<text start="0.4" dur="2.14">line</text>`.
  - Auto-generated captions often repeat cues; cleaning requires unescaping XML entities (`&amp;`, `&#39;`, `&quot;`) and collapsing identical consecutive text chunks.
- **Reference Implementation in `../oh-my-pi`:**
  - `packages/coding-agent/src/web/scrapers/github.ts`: URL parser, API fetching with `GITHUB_TOKEN`, Markdown formatting for issues, comments, commits, files, trees.
  - `packages/coding-agent/src/web/scrapers/youtube.ts`: URL parser, metadata formatting, caption extraction and cleaning.

## 2. Reuse

- **HTTP Transport:** Reuse `HttpClient` from `crates/rho-engine/src/tools/web/http/mod.rs` (configured with `no_proxy()`, DNS validation, redirect limiting, and timeout).
- **XML Parsing:** Reuse `quick-xml = "0.41"` (already a workspace dependency in `crates/rho-engine`) for stream-parsing YouTube timedtext XML without external binaries.
- **JSON Processing:** Reuse `serde` and `serde_json` for parsing GitHub API payloads and YouTube player responses.
- **HTML/Regex Utilities:** Reuse `scraper` and `regex` for extracting script blocks and URL patterns.
- **Caching & Pagination:** Reuse `FetchCache` and `pagination::format_page` in `crates/rho-engine/src/tools/web/fetch/` for consistent offset/limit handling and caching.

## 3. Invariants and Security Boundaries

- **Pure Domain & Adapter Separation:**
  - Extraction and Markdown generation logic (parsing URLs, formatting issues/PRs/commits, decoding caption XML) are pure, deterministic functions in `extract/github.rs` and `extract/youtube.rs`.
  - HTTP fetching and token resolution remain thin adapters in the respective modules.
- **Credential Safety:**
  - `GITHUB_TOKEN` / `GH_TOKEN` is used strictly as an HTTP `Authorization` header to `api.github.com` and is never logged, leaked in errors, or embedded in cache keys or Markdown output.
- **Public Network Isolation:**
  - All outbound requests continue to go through `HttpClient` URL validation and public DNS checks (`assert_public_dns`), preserving existing SSRF protections.
- **Zero External Subprocesses:**
  - All extraction is native Rust in-process. No execution of `yt-dlp`, `curl`, or `python`.

## 4. Quality Gates

- `cargo fmt --all -- --check`
- `make clippy` (or `cargo clippy --workspace --all-targets -- -D warnings`)
- `cargo test --workspace`
- `make crap` (CRAP score <= 30 on all modified/added functions)

## 5. Definition of Done

- Comprehensive unit tests covering:
  - GitHub URL parsing for issues, PRs, commits, blobs, trees, and unknown routes.
  - GitHub issue and PR rendering with metadata, description, and comments.
  - GitHub commit rendering with stats and file patch diffs.
  - GitHub blob and tree formatting.
  - YouTube URL parsing (`watch?v=`, `youtu.be/`, `shorts/`, `embed/`).
  - YouTube player response extraction (title, channel, duration, description).
  - YouTube timed-text XML parsing, unescaping, deduplication, and timestamp formatting.
  - Missing caption fallback to metadata and informative note.
  - Bypass with `format = "html"`.
- Clean compilation without clippy warnings or CRAP score violations.
- Documentation updated in `README.md` and `www/docs.html`.

## 6. Assumptions

- `GITHUB_TOKEN` or `GH_TOKEN` can be read from environment variables; if neither is set, unauthenticated GitHub API requests work within rate limits (60/hr).
- YouTube watch pages provide `ytInitialPlayerResponse` containing caption track metadata for videos with captions enabled.
- If captions are not available, providing full video metadata (title, channel, duration, description) and an explicit notice is significantly better than failing with an SPA shell error.

## 7. Risks & Mitigation

- **Risk:** GitHub unauthenticated rate limit reached (HTTP 403).
  - **Mitigation:** Detect 403 rate limit status and provide a clear, friendly error message explaining how to provide `GITHUB_TOKEN`.
- **Risk:** YouTube watch page HTML changes script tag layout.
  - **Mitigation:** Use multiple regex search patterns (`ytInitialPlayerResponse\s*=\s*(\{.+?\});`, `var ytInitialPlayerResponse\s*=\s*(\{.+?\});`) and handle missing player response gracefully by falling back to page `<meta>` extraction.

## 8. Dependencies

- No new dependencies. All required crates (`quick-xml`, `scraper`, `reqwest`, `serde`, `serde_json`, `url`, `regex`) already exist in `crates/rho-engine/Cargo.toml`.

## 9. Decisions

- **Decision 1: Native In-Process YouTube Extractor vs. `yt-dlp` CLI wrapper.**
  - *Options:* (A) Spawn `yt-dlp` process. (B) Native HTTP fetch of `ytInitialPlayerResponse` + timedtext XML.
  - *Choice:* (B) Native in-process.
  - *Tradeoffs:* Zero external runtime dependencies; faster execution; no need to install Python or binaries; handles timedtext directly via `quick-xml`.
- **Decision 2: Intercept at `fetch_and_extract` level vs. `extract_text` level.**
  - *Options:* (A) Intercept inside `extract_text` after raw HTML is fetched. (B) Intercept in `fetch_and_extract` before the generic HTTP fetch.
  - *Choice:* (B) Intercept in `fetch_and_extract`.
  - *Tradeoffs:* GitHub API requires specialized endpoints (`/issues/`, `/commits/`) rather than the raw HTML page, and blobs can be fetched directly from raw endpoints, saving bandwidth and avoiding rate limits.
- **Decision 3: Format Override Bypass.**
  - *Choice:* If `options.format_override == Some("html")`, skip specialized extractors and perform standard HTML scrape to allow users/agents to view the raw page if desired.

## 10. Out of Scope

- Writing to GitHub (submitting comments, stars, merges).
- Audio/video media stream downloading or local transcription via Whisper.
- Private GitHub repositories without a configured `GITHUB_TOKEN`.

---

## Tasks & Estimates

### Slice 1: URL Recognition & Extraction Types [Score: 2]
**Goal:** Implement robust URL parsing and representation for GitHub and YouTube URLs.

#### Task 1.1: Implement GitHub URL parser and types [2]
**Do:**
- Create `crates/rho-engine/src/tools/web/fetch/extract/github/mod.rs` (or `types.rs`/`url.rs`).
- Define `GitHubUrl` enum/struct:
  - `Issue { owner: String, repo: String, number: u64 }`
  - `PullRequest { owner: String, repo: String, number: u64 }`
  - `Commit { owner: String, repo: String, sha: String }`
  - `Blob { owner: String, repo: String, ref_name: String, path: String }`
  - `Tree { owner: String, repo: String, ref_name: Option<String>, path: String }`
  - `Repo { owner: String, repo: String }`
- Function `parse_github_url(url: &str) -> Option<GitHubUrl>`.
- Parse `/issues/{n}`, `/pull/{n}`, `/commit/{sha}`, `/blob/{ref}/{path}`, `/tree/{ref}/{path}`.
**Tests:** Table-driven unit tests for all GitHub URL variations, query parameters, anchors, and non-GitHub URLs.
**Verify:** `cargo test -p rho-engine tools::web::fetch::extract::github::url`

#### Task 1.2: Implement YouTube URL parser and types [2]
**Do:**
- Create `crates/rho-engine/src/tools/web/fetch/extract/youtube/mod.rs` (or `url.rs`).
- Define `YouTubeUrl`: `video_id: String`.
- Function `parse_youtube_url(url: &str) -> Option<YouTubeUrl>`.
- Support:
  - `youtube.com/watch?v=VIDEO_ID` (and `m.youtube.com/watch`)
  - `youtu.be/VIDEO_ID`
  - `youtube.com/embed/VIDEO_ID`
  - `youtube.com/v/VIDEO_ID`
  - `youtube.com/shorts/VIDEO_ID`
- Validate video ID pattern (11 alphanumeric, `_`, `-` characters).
**Tests:** Table-driven unit tests for all YouTube URL variants and negative non-video URLs.
**Verify:** `cargo test -p rho-engine tools::web::fetch::extract::youtube::url`

**Slice Verification:**
- Both parsers pass table-driven tests covering edge cases (query strings, trailing slashes, fragments).

---

### Slice 2: GitHub Specialized Extractor [Score: 3]
**Goal:** Fetch GitHub resources via REST API or raw endpoints, and render them as clean, token-efficient Markdown.

#### Task 2.1: GitHub API client and token resolution [2]
**Do:**
- Implement GitHub API request helper in `crates/rho-engine/src/tools/web/fetch/extract/github/client.rs`.
- Read token from `std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN"))`.
- Set headers:
  - `User-Agent: rho-agent`
  - `Accept: application/vnd.github.v3+json`
  - `Authorization: Bearer <token>` (if present)
- Handle HTTP 403 rate limits with a helpful message: `"GitHub API rate limit exceeded. Set GITHUB_TOKEN environment variable for higher rate limits."`
**Tests:** Unit tests verifying header generation and rate-limit error mapping.
**Verify:** `cargo test -p rho-engine tools::web::fetch::extract::github::client`

#### Task 2.2: Implement Issue and Pull Request Markdown formatters [3]
**Do:**
- Implement `render_issue` and `render_pull_request`.
- Fetch issue/PR JSON: title, state, user, created_at, updated_at, body, labels, comments_count.
- For PRs: fetch base/head branches, additions/deletions, and PR diff (via `.diff` or diff Accept header).
- Fetch issue comments with pagination up to 100 comments.
- Format Markdown:
  - Title and metadata badge: `# Title\n\n**#123** · open · opened by @user\n...`
  - Body markdown.
  - Comments section with `### @user · timestamp` and comment body.
  - Diff section for PRs.
**Tests:** Unit tests with mocked JSON payloads verifying generated Markdown formatting.
**Verify:** `cargo test -p rho-engine tools::web::fetch::extract::github::issue`

#### Task 2.3: Implement Commit, Blob, and Tree formatters [3]
**Do:**
- Implement `render_commit`: query `/commits/{sha}`, extract author, message, stats (+/-), and `files[].patch` formatted into ````diff` blocks.
- Implement `render_blob`: fetch raw file directly from `raw.githubusercontent.com/{owner}/{repo}/{ref}/{path}` using `HttpClient`.
- Implement `render_tree`: query `/contents/{path}?ref={ref}` and format directory listing table or bullet tree.
**Tests:** Unit tests verifying commit patch formatting and directory listing output.
**Verify:** `cargo test -p rho-engine tools::web::fetch::extract::github::commit`

**Slice Verification:**
- `cargo test -p rho-engine tools::web::fetch::extract::github` passes all unit tests.

---

### Slice 3: YouTube Specialized Extractor [Score: 3]
**Goal:** Extract video metadata and timed-text transcripts from YouTube videos and format into clean dialogue Markdown.

#### Task 3.1: YouTube player response extractor [2]
**Do:**
- Implement `crates/rho-engine/src/tools/web/fetch/extract/youtube/player.rs`.
- Extract `ytInitialPlayerResponse` JSON from watch page HTML using regex and JSON parsing.
- Extract `videoDetails`:
  - `title`, `author` (channel), `length_seconds`, `view_count`, `short_description`.
- Extract caption tracks list from `captions.playerCaptionsTracklistRenderer.captionTracks`.
- Helper to select best caption track (prefer English `en`, `en-US`, `en-GB`, or fallback to first available track).
**Tests:** Unit tests verifying player response JSON parsing and track selection.
**Verify:** `cargo test -p rho-engine tools::web::fetch::extract::youtube::player`

#### Task 3.2: YouTube timed-text XML parser & transcript formatter [3]
**Do:**
- Implement `crates/rho-engine/src/tools/web/fetch/extract/youtube/transcript.rs`.
- Fetch caption track XML from `baseUrl` using `HttpClient`.
- Parse XML elements `<text start="0.4" dur="2.14">...</text>` with `quick-xml`.
- Decode XML entities (`&amp;` -> `&`, `&#39;` -> `'`, `&quot;` -> `"`, `&lt;` -> `<`, `&gt;` -> `>`).
- Format seconds into `[MM:SS]` (or `[HH:MM:SS]` if >= 3600s).
- Deduplicate repetitive auto-generated lines and group into chronological transcript lines.
- Format complete Markdown:
  - Video title header `# Title`.
  - Channel, duration, views, video ID metadata.
  - Video description snippet.
  - `## Transcript (<lang>)` section with timestamped dialogue.
  - If captions are not available: output `*No transcript available for this video.*`.
**Tests:** Unit tests parsing sample timedtext XML (including escaped entities and auto-generated duplicates) and formatting into dialogue Markdown.
**Verify:** `cargo test -p rho-engine tools::web::fetch::extract::youtube::transcript`

**Slice Verification:**
- `cargo test -p rho-engine tools::web::fetch::extract::youtube` passes all unit tests.

---

### Slice 4: Pipeline Interception & Integration [Score: 3]
**Goal:** Connect GitHub and YouTube specialized extractors into `WebFetchTool`.

#### Task 4.1: Wire extractors into `fetch_and_extract` [3]
**Do:**
- In `crates/rho-engine/src/tools/web/fetch/mod.rs`:
  - In `fetch_and_extract(&self, url_str: &str, options: FetchOptions<'_>)`:
  - If `options.format_override == Some("html")`, skip interception.
  - Check if `parse_github_url(url_str)` matches: call `extract_github(client, github_url, timeout, max_bytes)`.
  - Check if `parse_youtube_url(url_str)` matches: call `extract_youtube(client, youtube_url, timeout, max_bytes)`.
  - If an intercepted extractor returns an error or unsupported subroute, fall back to standard HTML fetch.
  - Output integrates seamlessly with existing `FetchCache` and `pagination::format_page`.
**Tests:** Integration tests verifying that `WebFetchTool::execute` with mock HTTP servers or test URLs correctly dispatches to extractors and respects `format = "html"`.
**Verify:** `cargo test -p rho-engine tools::web::fetch`

**Slice Verification:**
- `cargo test -p rho-engine tools::web::fetch` passes all unit and integration tests.

---

### Slice 5: Documentation & Web Synchronization [Score: 2]
**Goal:** Update user documentation and website to reflect the specialized fetch capabilities.

#### Task 5.1: Synchronize documentation and website [2]
**Do:**
- Update `README.md`: document that `web_fetch` includes specialized extractors for GitHub (issues, PRs with diffs, commits, raw code) and YouTube (video metadata and timed transcripts).
- Update `www/docs.html`: add a section on specialized fetch extractors and GitHub token authentication (`GITHUB_TOKEN` / `GH_TOKEN`).
- Update `www/index.html` card description if appropriate.
**Verify:** `git diff docs/ README.md www/`

#### Task 5.2: Final Verification & Quality Gates [2]
**Do:**
- Run `cargo fmt --all -- --check`.
- Run `make clippy` (or `cargo clippy --workspace --all-targets -- -D warnings`).
- Run `cargo test --workspace`.
- Run `make crap` to ensure all new/modified functions have CRAP score <= 30.
**Verify:** All checks exit with status 0.

---

## Final Verification Checklist

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `make crap`
