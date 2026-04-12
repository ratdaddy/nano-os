# SAFETY Comment Audit

## Goal

Ensure every `unsafe` block with a non-obvious invariant has a preceding `// SAFETY:`
comment explaining why the operation is sound.

## Scope

All `unsafe` blocks and `unsafe impl`s in `src/` where the reason for soundness is
not self-evident from context. MMIO register reads/writes and inline `asm!` blocks
do not require comments. See `ref/coding-style.md` for the full guideline.

## Approach

Grep for `unsafe` blocks across the codebase and review each one:

```
grep -rn "unsafe" src/ --include="*.rs"
```

For each block, determine whether the invariant is obvious. If not, add a `// SAFETY:`
comment. Pay particular attention to:

- Raw pointer dereferences
- `transmute` calls
- `unsafe impl Send` / `unsafe impl Sync`
- Static mutable access
- Lifetime extension

## Note

The pre-commit checklist requires addressing safety comments in any file that is
touched during a commit, even for unsafe blocks that were not introduced in that diff.
This backlog item covers a one-time sweep of the whole codebase.
