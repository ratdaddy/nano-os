# ext2 test coverage

Evaluate test coverage for ext2 once the implementation stabilizes. The read
path is currently changing too rapidly for tests to be maintainable, but that
condition should be reassessed when it settles.

## When to revisit

When the ext2 read path (block reading, inode lookup, directory traversal,
indirect blocks) and write path interface are stable enough that tests won't
require constant rework.

## Related

`ext2_lookup_error_mapping.md` — error mapping bug worth addressing at the
same time.
