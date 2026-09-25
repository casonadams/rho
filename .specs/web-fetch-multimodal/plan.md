# Multimodal Web Fetch Analysis Fallback Plan

## 1. Research

The `web_fetch` tool currently handles HTML, Markdown, JSON, XML, CSV, and digital PDFs. When encountering direct image URLs (`.png`, `.jpg`, `.webp`, `.svg`), it throws an unsupported content type error or attempts invalid UTF-8 string decoding. When encountering scanned or complex PDFs, `pdf_extract::extract_text_from_mem` either throws an error or returns empty whitespace because scanned documents have no native text streams.

Gemini 2.5 Flash (`gemini-2.5-flash`) provides fast, low-cost multimodal analysis supporting direct `inlineData` for `image/png`, `image/jpeg`, `image/webp`, `image/gif`, and `application/pdf` (up to 20 MB). SVG diagrams can be passed directly as XML text within the prompt for structural analysis and OCR.

In rho:
- `crates/rho-engine/src/tools/web/search/gemini.rs` already demonstrates how to query the Gemini Developer API via `reqwest` using `GEMINI_API_KEY`.
- `crates/rho-engine/src/auth/` and `AuthStore` provide fallback credential storage under `"gemini"` and `"google"`.
- `crates/rho-engine/src/tools/web/fetch/` contains `WebFetchTool`, `FetchCache`, `encoding.rs`, and `extract/`.
- `crates/rho-harness-core/src/args.rs` defines `WebFetchArgs`.
- `crates/rho-harness-core/src/config/` manages `[tools.web.fetch]`.

## 2. Reuse

- **HTTP Client**: Reuse `HttpClient` from `crates/rho-engine/src/tools/web/http/` with its timeout and SSRF/private network protection.
- **Cache**: Reuse `FetchCache` from `crates/rho-engine/src/tools/web/fetch/cache.rs` to cache Gemini multimodal results.
- **Base64 Encoding**: Reuse `base64::engine::general_purpose::STANDARD` (already in workspace dependencies) for encoding binary image and PDF payloads.
- **Gemini API JSON Structure**: Follow Gemini Developer API conventions already utilized in `crates/rho-engine/src/tools/web/search/gemini.rs`.
- **Existing Native Extractor**: Keep `pdf_extract` as the primary local, zero-latency extractor for text-based PDFs.

## 3. Invariants and Security Boundaries

- **Credential Redaction**: Never include API keys in formatted Markdown, error messages, or cache keys.
- **SSRF / DNS Protection**: All remote resources are fetched through `HttpClient` with public DNS validation before being passed to Gemini. The Gemini client never initiates external URL downloads itself.
- **Payload Safety**: Enforce a strict 20 MB payload limit to stay within Gemini API bounds and prevent memory exhaustion.
- **Domain vs Transport Separation**: Multimodal payload creation, prompt templating, and response parsing are pure domain functions completely decoupled from TUI or CLI code.

## 4. Quality Gates

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings` (no suppressions permitted)
- `cargo test --workspace`
- `make crap` (all functions must maintain a CRAP score <= 30)

## 5. Definition of Done

- Detection logic identifies image MIME types (`image/png`, `image/jpeg`, `image/webp`, `image/gif`, `image/svg+xml`) and file extensions.
- Gemini multimodal client formats `generateContent` payloads with `inlineData` for binary images/PDFs and inline text for SVGs.
- Native PDF extraction automatically falls back to Gemini on errors or when extracted text is low-confidence (< 50 non-whitespace chars on documents >= 10 KB).
- Direct image URLs are automatically analyzed via Gemini multimodal pipeline when enabled.
- `format = "multimodal"` and `format = "image"` force multimodal routing.
- Configuration option `tools.web.fetch.multimodal` allows disabling the feature.
- All workspace tests and quality gates pass without warnings.

## 6. Assumptions

- `gemini-2.5-flash` is the standard Gemini model for fast, token-efficient multimodal analysis.
- When `GEMINI_API_KEY` (or `GOOGLE_API_KEY` or stored credential) is absent, image URLs return an actionable error informing the user how to configure credentials, while digital PDFs continue working natively.
- Documents > 20 MB will reject multimodal analysis before transmission to avoid HTTP 413 from Google APIs.

## 7. Risks

- **Risk**: Gemini API rate limit or outage during multimodal fetch.
  - **Mitigation**: For PDF extraction, preserve and return native extraction output if available; for images, return a clear, user-actionable error message without crashing.
- **Risk**: High latency on large documents.
  - **Mitigation**: Cache analysis in `FetchCache`; use `gemini-2.5-flash` which typically responds within 1-3 seconds.

## 8. Dependencies

- No new crate dependencies. All required crates (`reqwest`, `serde`, `serde_json`, `base64`) are already in workspace root and `rho-engine`.

## 9. Decisions

- **Decision**: Keep native `pdf_extract` as primary and Gemini as fallback.
  - *Context*: Text-based PDFs should remain instant, offline-capable, and free of API quota usage.
  - *Tradeoff*: Slight complexity in detecting low confidence, but saves API quota and latency on 90%+ of standard PDF fetches.
- **Decision**: Send SVG diagrams as XML text rather than rasterized images.
  - *Context*: Gemini understands XML/SVG diagrams natively when provided in text prompts, avoiding a heavy client-side SVG rasterizer crate dependency (like `resvg`).
- **Decision**: Centralize Gemini multimodal calling inside `crates/rho-engine/src/tools/web/fetch/multimodal.rs`.
  - *Context*: Keeps `fetch/mod.rs` cohesive and prevents file bloat, respecting repository instructions.

## 10. Out of Scope

- Audio/video transcription (YouTube is handled via specialized transcript extractor).
- Client-side headless browser rendering for client-side canvas charts.
- Local vision models (Ollama vision); cloud Gemini is the scope for Issue #39.

---

## Slices

### Slice 1: Multimodal Detection & Gemini Client

**Goal**: Build image format detection, Gemini multimodal request serialization, response parsing, and credential resolution with unit tests.

#### Task 1.1: Image detection and MIME classification [2]
**Do**: Add `is_image` and `image_mime_type` helpers in `crates/rho-engine/src/tools/web/fetch/encoding.rs` supporting PNG, JPEG, WebP, GIF, and SVG by MIME type and URL extension.
**Context**: Reuses existing regex and MIME inspection patterns in `encoding.rs`.
**Tests**: Unit tests for MIME types, extensions, and edge cases (query strings, capital letters).
**Verify**: `cargo test -p rho-engine --lib tools::web::fetch::encoding`

#### Task 1.2: Gemini multimodal payload serialization and response parsing [3]
**Do**: Create `crates/rho-engine/src/tools/web/fetch/multimodal.rs`. Implement:
- `build_image_payload(bytes, mime_type)` using `base64` and `inlineData`.
- `build_svg_payload(svg_text)`.
- `build_pdf_payload(bytes)` using `inlineData` with `application/pdf`.
- `parse_gemini_multimodal_response(json_str)` extracting Markdown text.
- Detection for low-confidence PDF text (`is_low_confidence_pdf(text, byte_len)`).
**Context**: Follows existing Gemini structure in `tools/web/search/gemini.rs`.
**Tests**: Unit tests in `multimodal.rs` verifying JSON structure for image, SVG, PDF, and response parsing.
**Verify**: `cargo test -p rho-engine --lib tools::web::fetch::multimodal`

#### Task 1.3: Gemini multimodal execution and API key resolution [3]
**Do**: Implement `analyze_multimodal(&HttpClient, &MultimodalRequest)` in `multimodal.rs`.
- Resolve API key from `GEMINI_API_KEY`, `GOOGLE_API_KEY`, or `auth.json`.
- Enforce 20 MB size limit.
- Post request to `gemini-2.5-flash:generateContent`.
- Handle HTTP errors with descriptive messages.
**Tests**: Unit tests for key resolution and error handling.
**Verify**: `cargo test -p rho-engine --lib tools::web::fetch::multimodal`

**Slice 1 Verification**: `cargo test -p rho-engine --lib tools::web::fetch` passes.

---

### Slice 2: Configuration & Argument Extension

**Goal**: Update configuration structures and `WebFetchArgs` to support multimodal options.

#### Task 2.1: Update `WebFetchArgs` schema and documentation [1]
**Do**: Update `WebFetchArgs` in `crates/rho-harness-core/src/args.rs` to document `"multimodal"` and `"image"` format overrides.
**Tests**: Verify schemars JSON schema generation.
**Verify**: `cargo test -p rho-harness-core --lib args`

#### Task 2.2: Add `multimodal` toggle to `tools.web.fetch` config [3]
**Do**: In `crates/rho-harness-core/src/config/`:
- Add `pub multimodal: bool` (default `true`) to `WebFetchConfig` in `types/app.rs`.
- Add `pub multimodal: Option<bool>` to `FileWebFetchConfig` in `types/file.rs`.
- Handle `multimodal` in `merge_file`, `storage.rs`, and key parsing.
**Tests**: Config parsing and serialization tests in `rho-harness-core`.
**Verify**: `cargo test -p rho-harness-core --lib config`

**Slice 2 Verification**: `cargo test -p rho-harness-core` passes.

---

### Slice 3: WebFetchTool Routing & Fallback Integration

**Goal**: Integrate multimodal analysis into `WebFetchTool::fetch_and_extract` for image URLs, format overrides, and scanned PDF fallbacks.

#### Task 3.1: Pass configuration and auth context to `WebFetchTool` [2]
**Do**: Update `WebFetchConfig` in `crates/rho-engine/src/tools/web/fetch/mod.rs` to include `multimodal_enabled: bool` and optional `auth_file: Option<PathBuf>`. Update `builtin_tools/mod.rs` to propagate these from `Config`.
**Tests**: Existing `WebFetchTool` instantiation tests updated.
**Verify**: `cargo test -p rho-engine --lib tools::builtin_tools`

#### Task 3.2: Implement direct image routing and PDF fallback in `fetch_and_extract` [3]
**Do**: In `crates/rho-engine/src/tools/web/fetch/mod.rs`:
- Detect if response is an image: route to `analyze_multimodal`.
- For PDFs: run native `extract_pdf_bytes`. If native extraction fails or yields low confidence (< 50 chars for >= 10 KB file), and multimodal is enabled, call `analyze_multimodal`.
- If `format_override` is `"multimodal"` or `"image"`, route directly to `analyze_multimodal`.
- Cache multimodal Markdown in `FetchCache`.
**Tests**: Integration tests in `crates/rho-engine/src/tools/web/fetch/tests.rs` verifying image routing, PDF fallback, format override, and disabled config behavior.
**Verify**: `cargo test -p rho-engine --lib tools::web::fetch`

**Slice 3 Verification**: `cargo test -p rho-engine --lib tools::web::fetch` passes.

---

### Slice 4: Documentation & Parity Verification

**Goal**: Synchronize documentation, website pages, and verify all quality gates.

#### Task 4.1: Update documentation and website [2]
**Do**:
- Update `docs/configuration.md` with `tools.web.fetch.multimodal`.
- Update `docs/tools.md` describing image and scanned PDF capabilities for `web_fetch`.
- Update `www/docs.html` (or relevant website docs) to maintain documentation parity.
**Verify**: Review diff with `git diff docs/ www/`.

#### Task 4.2: Full workspace quality gate audit [2]
**Do**: Run full formatting, clippy, test suite, and CRAP score check. Fix any lints or complexity regressions.
**Verify**:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
make crap
```

---

## Final Verification

1. `cargo fmt --all -- --check` -- zero formatting differences.
2. `cargo clippy --workspace --all-targets -- -D warnings` -- zero warnings or suppressions.
3. `cargo test --workspace` -- all unit and integration tests passing.
4. `make crap` -- all functions have a CRAP score <= 30.
5. Direct verification with mock HTTP server testing:
   - PNG image fetch returns structured Markdown summary when Gemini is configured.
   - Scanned PDF fetch with 0 native characters successfully falls back to Gemini multimodal OCR.
   - Normal text PDF fetch uses native `pdf_extract` without invoking Gemini.
