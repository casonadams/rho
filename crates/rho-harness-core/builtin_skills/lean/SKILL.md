---
name: lean
description: Apply minimalist discipline to a coding task: YAGNI ladder, smallest diff, one runnable check.
argument-hint: "<task-description>"
---

# Lean Task Discipline

Apply this discipline to the current task. Read the task and the code it touches fully first - trace the real flow end to end before picking a rung. Laziness that skips comprehension ships a confident wrong fix; that is the dangerous kind.

## The Ladder

Stop at the first rung that holds:

1. Does this need to exist at all? Speculative need = skip it, say so in one line (YAGNI).
2. Does this codebase already have it? Reuse the existing helper, type, or pattern; re-implementing what is a few files over is the most common slop.
3. Does the standard library do it? Use it.
4. Does a native platform feature or already-installed dependency cover it? Use it. Never add a new dependency for what a few lines can do.
5. Can it be one line? One line.
6. Only then: the minimum code that works.

Two stdlib options, same size? Take the one that is correct on edge cases - lazy means writing less code, not picking the flimsier algorithm.

## Bug Fixes

Fix the root cause, not the symptom. A report names a symptom: before editing, check every caller of the function you are about to touch. One guard in the shared function beats a guard in every caller, and patching only the path the report names leaves sibling callers broken.

## Rules

- Smallest coherent change that solves the root problem. Keep diffs focused; never mix unrelated cleanup or reformatting.
- No unrequested abstractions: no interface with one implementation, no factory for one product, no config for a value that never changes.
- No boilerplate, scaffolding "for later", dead code, or unrequested TODOs.
- Deletion over addition. Boring over clever - clever is what someone decodes at 3am.
- Fewest files possible. Do not fragment straightforward logic into tiny helpers that obscure control flow.
- Complex request? Ship the lazy version and question it in the same response ("Did X; Y covers it. Need full X? Say so."). Never stall on an answer you can default.

## Never Simplify Away

- Understanding the problem (read fully first).
- Input validation at trust boundaries.
- Error handling that prevents data loss.
- Security measures and accessibility basics.
- Anything the user explicitly asked for - user insists on the full version, build it, no re-arguing.
- Non-trivial logic (a branch, a loop, a parser, a money/security path) leaves ONE runnable check behind: the smallest thing that fails if the logic breaks. No frameworks unless the repo already uses them. Trivial one-liners need no test.

## Output

Code first. Then at most three short lines: what was skipped, when to add it. If the explanation is longer than the code, delete the explanation - unless the user explicitly asked for a walkthrough or report, then give it in full. State deliberate simplifications that cut a real corner with a known ceiling plainly (e.g. "global lock; per-account locks if throughput matters"). Keep the project's existing conventions, lint rules, and comment policy intact - lean applies to what gets built, not to lowering the project's own standards.

This discipline governs how code gets built. Answer questions and discuss normally.