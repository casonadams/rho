# Specification: Language Server Protocol (LSP) Intelligence Tool

**Status**: Draft  
**Target Milestone**: v0.11.x  
**Affected Crates**: `rho-engine`, `rho-harness-core`

---

## 1. Problem Statement
Agents currently inspect code using text tools (`read`, `rg`, `fd`). When an agent edits a file or renames a symbol, it has no native semantic feedback until it executes a slow compiler build (`cargo check`, `tsc`, `go test` via `bash`).
- **Delayed Error Loops**: A typo in an import or type signature only surfaces minutes later after a heavy build step.
- **Fragile Refactorings**: Renaming a method across 20 files requires tedious searching and multiple edit rounds that often miss call sites or re-exports.

## 2. Proposed Solution: Native `lsp` Tool
Integrate an in-process JSON-RPC client capable of speaking to standard Language Server Protocol (LSP) servers already present in the developer's environment (e.g., `rust-analyzer`, `typescript-language-server` / `biome`, `gopls`, `pyright`, `clangd`).

### Capabilities Exposed to the Agent
- `diagnostics`: Query compilation errors, type mismatches, and warnings on a file or workspace after an edit.
- `definition`: Jump directly to symbol definitions across crates/packages.
- `references`: Find all references of a function, struct, or variable.
- `rename`: Perform semantic workspace-wide symbol renames through `workspace/willRenameFiles` and text edits before touching disk.
- `symbols`: Search workspace or document symbol trees.

## 3. Tool Interface

```json
{
  "operation": "diagnostics" | "definition" | "references" | "rename" | "symbols",
  "path": "src/main.rs",
  "line": 42,
  "character": 15,
  "new_name": "renamed_symbol"
}
```

## 4. Implementation Steps
1. Add lightweight asynchronous LSP JSON-RPC client in `rho-engine/src/lsp/`.
2. Discover auto-configured language servers based on project root marker files (`Cargo.toml` -> `rust-analyzer`, `package.json` -> `typescript-language-server`, `go.mod` -> `gopls`).
3. Add `lsp` tool definition to `rho-engine/src/tools/`.
4. Surface instant post-edit diagnostic summaries directly in the TUI edit cards.
