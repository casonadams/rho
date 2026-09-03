---
name: simplify
description: Review code or changes for over-engineering to delete; one line per finding.
argument-hint: "[path-or-directory]"
---

# Simplify Review

Report what to cut. This is a one-shot read-only review - do not modify files unless the user asks you to apply the findings.

## 1. Determine the Target

- With a path argument: review that file or directory tree.
- With no argument: review the current uncommitted changes (`git status --short` and `git diff HEAD`) - review the changed files in their current state, not just the diff hunks.
- If there are no changes and no path, say so and stop.

## 2. Hunt for Over-Engineering

Read the target files and look for:

- **Reinvented stdlib / platform features**: hand-rolled utilities that mirror standard library functions or native platform capabilities.
- **Replaceable dependencies**: a dependency doing what a few lines - or an already-installed dependency - could do. Weigh maintenance, security, licensing, and size.
- **Speculative abstraction**: interfaces with one implementation, factories with one product, config for values that never change, extension points nothing extends, generics used once.
- **Dead code**: unused functions, flags, params, exports; placeholder TODOs; commented-out code.
- **Duplication**: the same logic in multiple places that belongs in one shared function (especially if a bug fix already patched only some copies).
- **Over-fragmentation**: tiny helpers or files that obscure control flow instead of clarifying it; pass-through wrappers adding nothing.
- **Defensive bloat**: fallbacks that swallow errors, validation far from trust boundaries, retries/limits nothing asked for.

Only flag things within the target scope. Do not propose rewrites of working code that is merely styled differently from how you would write it.

## 3. Report

One line per finding, ranked by lines removable (largest first):

```
path:line - what to cut -> what replaces it (~N lines)
```

Group by file. End with the total count and total estimated lines removable. If the target is clean, say so in one line - do not invent findings.

## 4. Applying (only when asked)

Apply findings one by one, smallest first. Run the project's test suite between each removal and stop on any failure. Never remove security checks, input validation at trust boundaries, error handling that prevents data loss, or accessibility behavior - flag those as intentional even in a simplify pass.