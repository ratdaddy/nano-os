# Testing Strategy

## Coverage goals

Not aiming for 100% coverage. Tests should focus on code with **logic and error handling** — places where things can go wrong or where behavior needs to be verified.

## What to test

- Functions with branching logic, loops, or error detection
- Error paths that return distinct error types (e.g., `NotFound` vs `NotADirectory`)
- Edge cases in parsing or data accumulation (e.g., chunked reads, EOF handling)

## What not to test

- Pure pass-through functions that delegate to another layer with no transformation (e.g., `vfs_read` just calls `ops.read` — no logic to test)
- Trivial getters or wrappers

## Unit test isolation

Tests for a module should test the **logic in that module**, not in its dependencies. If a function's behavior depends on a downstream implementation (e.g., VFS depends on filesystem `lookup` returning specific errors), use a **mock/test harness** rather than the real implementation.

This ensures:
- Tests fail because of bugs in the module under test, not in a dependency
- Tests document the contract the module expects from its dependencies
- Tests remain fast and focused

### Example: VFS tests

`vfs_open` splits paths and iterates `lookup` calls — that's VFS logic worth testing. But the errors come from the `Inode` implementation, so VFS tests use a `MockInode` that returns configured errors rather than testing through ramfs.

The real filesystem (ramfs) needs its own tests verifying it returns the correct error types from `lookup`.

## Error type correctness

Error variants should map to distinct failure modes that callers need to distinguish, following POSIX conventions where applicable:

- `NotFound` (ENOENT) — path component doesn't exist
- `NotADirectory` (ENOTDIR) — traversing through or operating on a non-directory
- `InvalidInput` (EINVAL) — generic invalid argument
- `UnexpectedEof` — data shorter than expected
- `InvalidUtf8` — byte sequence is not valid UTF-8

Tests should assert the **specific error variant**, not just that an error occurred.
