# Replace static mut globals with Mutex

## Problem

Several global statics use `static mut` with `Option` wrapping and `addr_of!`/`addr_of_mut!`
access patterns. This requires `unsafe` blocks at every access site and manual safety
reasoning that is easy to get wrong. For registration-style globals (written once at boot,
then read-only), a `Mutex` is strictly better: no unsafe, compiler-enforced invariants,
and SMP-safe.

Overhead is negligible — `spin::Mutex` with no contention is a few atomic ops. These
globals are accessed rarely (boot time or per-syscall at most).

## Status

- [x] `src/dev/block.rs` — `BLOCKDEVS` converted to `Mutex<BTreeMap<(u32, u32), BlkdevEntry>>`

## Remaining

| File | Static | Current type |
|---|---|---|
| `src/dev/char.rs:13` | `CHARDEVS` | `Option<BTreeMap<(u32, u32), CharDevEntry>>` |
| `src/vfs.rs:33` | `MOUNTS` | `Option<Vec<Mount>>` |
| `src/vfs.rs:94` | `FILESYSTEMS` | `Option<Vec<&'static dyn FileSystem>>` |
| `src/kprint.rs:14` | `CONSOLE` | `Option<File>` |

## Pattern to apply

Replace:
```rust
static mut FOO: Option<BTreeMap<K, V>> = None;
```

With:
```rust
static FOO: Mutex<BTreeMap<K, V>> = Mutex::new(BTreeMap::new());
```

And replace all `addr_of!`/`addr_of_mut!` + `unsafe` access blocks with direct
`FOO.lock()` calls. Drop the `Option` entirely when the type has a sensible default
(empty collection, etc.).

## Notes on CONSOLE

`kprint.rs:CONSOLE` is called on every `kprintln!` so it is accessed more frequently
than the others. On a single CPU with no contention the Mutex cost is still negligible,
but worth noting. The `Option` there signals "not yet initialized" — after conversion
the initialization check moves into the lock guard.
