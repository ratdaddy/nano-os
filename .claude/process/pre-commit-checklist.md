# Pre-Commit Checklist — nano-os

Repo-specific items checked by `/pre-commit-check` after the global steps.

## Trait checklist

Apply the Review Procedure from `docs/ref/rust-trait-checklist.md` to every type
defined in every touched file — including types that were not directly changed.

## Code style

Work through the Code Review Checklist at the bottom of `docs/ref/coding-style.md`.

## Unsafe blocks

Every `unsafe` block with a non-obvious invariant must have a preceding `// SAFETY:`
comment explaining why the operation is sound. Check all unsafe blocks in every touched
file — not only those introduced in the current diff.

## Gate 3 applicability

If the diff touches boot-level behavior, device initialization, interrupt handling, or
anything not reachable by unit tests, Gate 3 (`make run`) is required before this commit
is complete. Flag it explicitly so the developer knows to run it before staging.
