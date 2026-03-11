# Replace UnsafeCell with RefCell in ramfs

## Current situation

`RamfsNode::Dir` uses `UnsafeCell<BTreeMap<String, Arc<Inode>>>` for its children map.
The intent is "mutate during init, read-only after mount" but the type system does not
enforce that transition. Both init-time writers (`insert_file`, `insert_dir`) and
post-mount readers (`lookup`, `readdir`) reach into the cell with raw pointer casts,
relying on a convention rather than any compile-time or runtime check.

## Desired change

Replace `UnsafeCell` with `RefCell`:

```rust
Dir { children: RefCell<BTreeMap<String, Arc<Inode>>> },
```

- Read sites use `children.borrow()` instead of `unsafe { &*children.get() }`
- Write sites use `children.borrow_mut()` instead of `unsafe { &mut *children.get() }`
- No `unsafe` at any call site
- `unsafe impl Send/Sync for RamfsNode` remains (RefCell is !Sync) but the comment
  becomes honest: aliasing is checked at runtime rather than assumed correct

If the "init then read-only" invariant is ever violated, the code panics instead of
causing undefined behaviour.

## Future: SMP

When multiple CPUs are supported, `RefCell` should be replaced with
`spin::Mutex<BTreeMap<...>>` and the `unsafe impl Sync` can be removed entirely.
