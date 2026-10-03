# Specification: Hashline & Content-Hashed Editing

**Status**: Draft  
**Target Milestone**: v0.10.x  
**Affected Crates**: `rho-engine`, `rho-harness-core`

---

## 1. Problem Statement
The current `edit` tool in `rho` relies on exact string replacement (`oldText` / `newText`). While intuitive for large frontier models (like Claude Sonnet 3.7 or GPT-4o), it degrades severely on smaller or faster models (Grok Code, Gemini Flash, DeepSeek V3, Qwen):
- **Whitespace / Tab Mismatches**: Small formatting discrepancies between what the model perceived and what is on disk cause exact string matches to fail.
- **Ambiguous / Duplicate Matches**: Models frequently fail to provide sufficient context around a replacement, requiring round-trip retries (`edits[].oldText matched 0 times` or `matched 3 times`).
- **Token Inefficiency**: Models must reproduce substantial surrounding context in `oldText` to achieve uniqueness, wasting output bandwidth and latency.

## 2. Proposed Solution: Hashline / Tagged Line Editing
Adopt a line-anchored patch language inspired by `oh-my-pi`:
1. Every file read via `read` or `rg` includes a compact 4-character content snapshot hash (e.g. `[src/main.rs#A1B2]`).
2. When performing an edit, the model specifies operations against line ranges or AST blocks anchored to that snapshot tag.
3. The parser validates the tag. If the file has changed on disk since the read, it halts with an explicit divergence error before mutating disk.
4. Support compact grammar:
   - `PUT N.=M:`: Replace inclusive lines `N` through `M` with subsequent `+<line>` rows.
   - `PUT <N:` / `PUT >N:`: Insert before or after line `N`.
   - `PUT >$:`: Append to end of file.
   - `CUT N.=M`: Delete lines `N` through `M`.
   - `MV <DEST>`: Atomic move/rename in the same patch block.

## 3. Schema & Wire Compatibility
To maintain backwards compatibility, the `edit` tool can accept either:
- The classic exact replacement schema: `{ path: string, edits: [{ oldText: string, newText: string }] }`
- The hashline patch payload: `{ path?: string, patch: string }` or an input string format.
A configuration option `[tools.edit] mode = "auto" | "hashline" | "exact"` will govern prompt instructions.

## 4. Implementation Steps
1. **Snapshot Tagging**: Update `read` tool output to attach a 4-hex xxHash/CRC32 checksum of file content at the top header (`[path#TAG]`).
2. **Patch Parser (`rho-engine/src/tools/edit/hashline.rs`)**:
   - Implement zero-allocation streaming parser for `PUT`, `CUT`, `MV` operations.
   - Line numbering mapped relative to the original snapshot snapshot buffer.
3. **Validation & Atomic Apply**:
   - Verify current file checksum matches snapshot tag.
   - Apply line substitutions in-memory, generate unified diff for presenter, and flush atomically via `tempfile` rename.
4. **Prompt Tuning & Guidance**:
   - Provide role-specific system prompt examples when hashline mode is active.
