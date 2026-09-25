# Multimodal Web Fetch Analysis Fallback Spec

## Status

Approved

## Problem

The `web_fetch` tool is designed to retrieve and convert web content into clean, readable Markdown for agent context. However, it currently suffers from severe limitations on non-text and complex visual documents:

1. **Direct Image URLs**: URLs pointing to images, diagrams, charts, or infographics (PNG, JPEG, WebP, SVG) immediately fail with `Unsupported content type: image/...` or corrupt byte decoding, leaving the agent blind to visual documentation, architecture diagrams, benchmark charts, and schematics.
2. **Scanned and Complex PDFs**: The native PDF extractor (`pdf_extract`) parses text streams from PDF objects. When a document is scanned, rasterized, uses non-standard font encodings, or contains complex tables and diagrams, native extraction either crashes (`PDF extraction error`), emits unreadable garbage, or yields empty whitespace without any text.
3. **No Multimodal Bridge in Web Pipeline**: Even though Gemini 2.5 Flash provides fast, low-cost multimodal reasoning, OCR, and document understanding, `web_fetch` has no pipeline to leverage it for visual media or failed text extractions.

Issue #39 requests an optional multimodal extraction pipeline using Gemini to analyze direct images, charts, and complex/scanned PDFs, returning structured Markdown summaries, tables, and descriptions into the agent's context.

## Users and Stakeholders

- **Agent / LLM Tool Calling**: Can fetch URLs referencing charts, screenshots, diagrams, and scanned PDFs to extract precise tabular data, diagram flows, and text content.
- **Developer / User**: Can ask questions about documentation containing image charts, infographics, architecture diagrams, or scanned whitepapers without manual transcription.
- **Tool Runtime**: Preserves low latency and zero external API costs for standard text/HTML/PDF pages, invoking Gemini only when necessary or explicitly requested.

## Goals

- **Image Extraction**: Intercept and analyze direct image URLs (`image/png`, `image/jpeg`, `image/webp`, `image/gif`, `image/svg+xml`) using Gemini 2.5 Flash, generating structured Markdown descriptions, data tables, and OCR transcriptions.
- **Scanned / Complex PDF Fallback**:
  - Retain native `pdf_extract` as the primary local, zero-cost, zero-network extractor.
  - Automatically fall back to Gemini multimodal extraction when native PDF extraction returns an error, produces empty text, or yields low confidence (< 50 non-whitespace characters on files >= 10 KB).
- **Conditional / Explicit Override**:
  - Support explicit format overrides `format = "multimodal"` or `format = "image"` in `WebFetchArgs` to force multimodal analysis on any document or image.
  - Allow bypassing multimodal analysis if disabled via configuration (`tools.web.fetch.multimodal = false`).
- **Structured Markdown Output**: Return clean GFM Markdown with clear metadata headers, reconstructed data tables (`| Col1 | Col2 |`), OCR transcriptions, and visual hierarchy.
- **Performance, Caching & Pagination**:
  - Cache multimodal analysis results in `FetchCache` so repeated fetches or line-paginated offsets (`offset`, `limit`) do not duplicate Gemini API calls.
  - Enforce payload size safety limits (up to 20 MB inline limit for Gemini API).
- **Zero New Dependencies**: Implement the Gemini API payload generation and response parsing using existing workspace dependencies (`reqwest`, `serde`, `serde_json`, `base64`).

## Non-goals

- Downloading raw binary media to local files via `web_fetch` (rho's `web_fetch` returns readable text context).
- Client-side headless browser rendering of JavaScript-only canvas charts (canvases rendered on-the-fly inside complex SPAs require browser automation, not direct HTTP fetch).
- Video/audio multimodal processing in `web_fetch` (YouTube transcripts are handled natively via the specialized YouTube extractor).
- Running local multimodal models (e.g. Ollama vision models) as part of this slice; Gemini Developer API provides the primary multimodal cloud fallback.

## Current Behavior

- In `crates/rho-engine/src/tools/web/fetch/mod.rs`:
  - `fetch_and_extract` checks `encoding::is_pdf`. If true, calls `extract::extract_pdf_bytes(resp.body)`.
  - If `extract_pdf_bytes` errors, it returns `AppError::Tool(format!("PDF extraction error: {e}"))`. If it yields empty string (e.g. scanned PDF), it returns an empty string which formats to `"[Empty content returned from URL]"`.
  - For non-PDF responses, it calls `encoding::decode_body` followed by `extract::extract_text`.
  - In `extract::extract_inferred_text`, if `content_type` is not text/html/json/xml/csv/markdown, it returns `Err(AppError::Tool("Unsupported content type: ..."))`.
- In `crates/rho-harness-core/src/args.rs`:
  - `WebFetchArgs.format` documentation only lists `"html", "json", "markdown", "csv", "xml", "pdf"`.
- In `crates/rho-harness-core/src/config/`:
  - `tools.web.fetch` only contains `enabled: bool`.

## Desired Behavior

- In `crates/rho-engine/src/tools/web/fetch/mod.rs`:
  - Detect image content types (`image/png`, `image/jpeg`, `image/webp`, `image/gif`, `image/svg+xml`) and file extensions (`.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`, `.svg`).
  - When an image URL is fetched:
    - If `tools.web.fetch.multimodal` is enabled (default `true`) and Gemini credentials are available (`GEMINI_API_KEY`, `GOOGLE_API_KEY`, or `auth.json`), call Gemini multimodal analysis.
    - If Gemini credentials are missing, return an informative error guiding the user to configure `GEMINI_API_KEY` or run `rho login gemini`.
  - When a PDF URL is fetched:
    - Attempt native PDF extraction first.
    - If native extraction succeeds with high confidence (>= 50 chars non-whitespace, or small file), use native text.
    - If native extraction fails or yields low-confidence text (< 50 chars on file >= 10 KB), and Gemini credentials are available, automatically invoke Gemini multimodal extraction as a fallback.
    - If `options.format_override == Some("multimodal")` or `Some("image")`, invoke Gemini directly without native extraction.
  - Return structured Markdown with data tables and descriptions.
  - Store results in `FetchCache` for standard pagination support.

## Requirements

- **REQ-001 (Image Detection)**: Identify image payloads by HTTP `Content-Type` header (`image/png`, `image/jpeg`, `image/webp`, `image/gif`, `image/svg+xml`) or URL file extension (`.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`, `.svg`).
- **REQ-002 (Gemini Multimodal Client)**: Implement a Gemini multimodal caller in `crates/rho-engine/src/tools/web/fetch/multimodal.rs` that:
  - Supports `image/png`, `image/jpeg`, `image/webp`, `image/gif`, and `application/pdf` via `inlineData` (base64-encoded).
  - Supports `image/svg+xml` and SVG documents by passing raw SVG XML text in the prompt.
  - Uses the `gemini-2.5-flash` model endpoint (`https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent`).
  - Sets appropriate analysis prompts tailored to extracting structured Markdown, data tables, and OCR text.
- **REQ-003 (Credential Resolution)**:
  - Resolve API keys checking `GEMINI_API_KEY` first, then `GOOGLE_API_KEY`, then persisted credentials in `auth.json` (under `"gemini"` or `"google"`).
  - Redact API keys from all error logs and cache keys.
- **REQ-004 (PDF Extraction Fallback Routing)**:
  - Retain `pdf_extract` for initial extraction.
  - Trigger fallback to Gemini multimodal extraction when:
    1. Native extraction returns `Err(_)`; OR
    2. Native extraction returns `Ok(text)` where `text.trim().len() < 50` and the PDF byte length is >= 10,240 bytes; OR
    3. `format_override` is explicitly set to `"multimodal"` or `"image"`.
- **REQ-005 (Configuration & Disabling)**:
  - Add `multimodal: Option<bool>` to `[tools.web.fetch]` in configuration (defaulting to `true` when omitted).
  - If `multimodal` is explicitly `false`, do not call Gemini; return native extraction results or unsupported content type errors.
- **REQ-006 (Format Override Support)**:
  - Accept `format = "multimodal"` and `format = "image"` in `WebFetchArgs`.
- **REQ-007 (Size Limits & Error Resilience)**:
  - Enforce payload limit of 20 MB for Gemini multimodal requests.
  - If a Gemini request fails (e.g. rate limit, network timeout, quota exceeded):
    - For PDF fallback: return the original native extraction result or a clear error indicating both native and Gemini attempts failed.
    - For image fetch: return a clear error stating Gemini multimodal analysis failed.

## Invariants and Security Boundaries

- **Credential Safety**: Never leak `GEMINI_API_KEY` or `GOOGLE_API_KEY` in tool output, Markdown responses, or error messages.
- **SSRF & Private Network Isolation**: Multimodal fetch must only analyze bytes retrieved through `HttpClient`, which enforces public DNS and private network restrictions. The Gemini client must not fetch target URLs directly.
- **Cost & Rate Control**: Multimodal API calls must be cached in `FetchCache` alongside standard fetch responses. Native extraction must remain the zero-cost default for digital PDFs.
- **Separation of Concerns**: Multimodal payload building and Gemini response parsing must remain pure functions independent of TUI or CLI layers.

## Definition of Done

- Unit tests cover:
  - Image MIME and extension detection (`is_image`, `image_mime_type`).
  - Low-confidence PDF detection logic.
  - Multimodal request payload construction for binary images, SVG XML, and PDFs.
  - Gemini response parsing for candidates, text parts, and error responses.
  - Fallback logic: error recovery when native PDF extraction returns empty text or fails.
- Integration tests in `crates/rho-engine/src/tools/web/fetch/tests.rs` verify routing through `WebFetchTool`.
- All quality gates pass: `cargo fmt --all -- --check`, `make clippy`, `cargo test --workspace`, and `make crap`.
- Documentation in `docs/` and `www/` updated to describe image and scanned PDF multimodal fetch capabilities.

## Acceptance Criteria

- AC-001: Given a URL returning `image/png` or `image/jpeg`, when `web_fetch` is called with Gemini configured, it returns structured Markdown describing the image and transcribing any visible text/tables.
- AC-002: Given a URL returning an image when no Gemini API key is configured, `web_fetch` returns a clean error informing the user that `GEMINI_API_KEY` is required for image analysis.
- AC-003: Given a scanned PDF with no extractable text stream, when `web_fetch` is called with Gemini configured, it falls back to Gemini and returns the OCR-extracted Markdown.
- AC-004: Given a digital text-rich PDF, `web_fetch` completes via native `pdf_extract` without querying Gemini.
- AC-005: Given `format: "multimodal"` on any PDF, `web_fetch` skips native extraction and directly uses Gemini multimodal analysis.
- AC-006: Given an SVG image URL, `web_fetch` extracts and analyzes the SVG diagram structure and text without binary decode errors.
- AC-007: Given `tools.web.fetch.multimodal = false` in `config.toml`, multimodal fallback is bypassed.

## Edge Cases

- **Empty / 0-byte images**: Detect early and return an error before making an API call.
- **SVG with XML entity bombs**: Handled by passing text to Gemini with safe length limits.
- **Oversized files (>20 MB)**: Reject before sending to Gemini API with an informative error.
- **Gemini rate limiting (HTTP 429)**: Return clear error indicating Gemini rate limit exceeded.
- **Native PDF extraction produces pure whitespace**: Correctly identified as low-confidence and routed to fallback.

## Constraints

- Pure Rust implementation using existing workspace dependencies (`reqwest`, `serde`, `serde_json`, `base64`).
- CRAP score <= 30 on all new and modified functions.
- Function parameter counts <= 6.

## Risks and Mitigations

- **Risk**: High latency on large PDFs or images during Gemini generation.
  - **Mitigation**: Use `gemini-2.5-flash` for fastest latency; cache results in `FetchCache`.
- **Risk**: Gemini API changes or errors.
  - **Mitigation**: Graceful fallback to native extraction result when available; clear error reporting.

## References

- Issue #39: `https://github.com/casonadams/rho/issues/39`
- Gemini Developer API `generateContent` documentation
- Existing Gemini search implementation: `crates/rho-engine/src/tools/web/search/gemini.rs`
- Existing PDF extraction: `crates/rho-engine/src/tools/web/fetch/extract/data.rs`
