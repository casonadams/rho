# Specification: Terminal Session Recording & Playback (`/record`, `rho play`)

**Status**: Draft  
**Target Milestone**: v0.12.x  
**Affected Crates**: `rho`, `rho-harness-core`

---

## 1. Problem Statement
Debugging agent misbehavior, reproducing edge cases in TUI rendering, sharing terminal sessions with colleagues, or auditing long-running autonomous workflows is challenging when only raw log files or JSONL transcripts exist. Static text misses dynamic TUI frame updates, progress spinners, tool status cards, and layout shifts.

## 2. Proposed Solution: Screen-Level Frame Recording (`.rhocast`)
Implement an in-terminal recorder and player inspired by asciinema and `oh-my-pi`'s `.ompcast`:
1. **`/record` Slash Command**:
   - Toggles recording in an active interactive session.
   - Captures normalized terminal rows (scrollback commits + live viewport updates) with millisecond timestamps into `<tmpdir>/rho-recordings/<timestamp>-<session_id>.rhocast`.
   - The TUI footer indicates `● REC` while recording.
2. **Deterministic Redaction**:
   - Every recorded frame passes through an irreversible secret redaction filter before hitting disk: masking environment secrets, bearer tokens, API keys, password shapes, and user-specified regexes (`••••••`).
3. **`rho play` Player**:
   - Standalone CLI command: `rho play <file.rhocast> [flags]`.
   - Supports `-s <speed>` (playback speed multiplier) and `-i <max-idle>` (pauses capped at `N` seconds).
   - Interactive playback controls: `Space` to pause/resume, `q`/`Esc` to exit, Left/Right arrow scrub.

## 3. Implementation Steps
1. Add frame serialization structures to `rho-harness-core/src/presentation/recording.rs`.
2. Hook frame capture into the synchronized paint pipeline in `src/ui/interactive/controller/`.
3. Implement `rho play` subcommand in `src/cli/play.rs`.
