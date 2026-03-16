# Rust Trait Implementation Checklist

This document is a pre-commit checklist for reviewing trait implementations in application code.
Review **all types in any file that has been added or changed** — not just the types that were
directly modified. A touched file is an opportunity to bring all types in it into compliance.

---

## Mandatory Traits — Apply Without Asking

These traits should be present on every qualifying type. If they are missing, add them.

### `Debug` — Every Type

Derive `Debug` on every struct and enum without exception.

```rust
#[derive(Debug)]
struct MyType { ... }
```

There is no meaningful cost. The absence causes pain in log output, panic messages, and test
failures. Do not skip this.

### `Display` + `Error` — Every Error Type

Any type that represents an error must implement both. Use `thiserror` to derive them.

```rust
#[derive(Debug, thiserror::Error)]
enum MyError {
    #[error("thing went wrong: {0}")]
    Thing(String),
}
```

If a type has "Error" or "Err" in its name, or is used as an error variant, this rule applies.

### `From` — Every Error Type Used Across Module Boundaries

Error types should implement `From` for the error types they wrap, so that `?` works without
explicit mapping. `thiserror`'s `#[from]` attribute handles this:

```rust
#[derive(Debug, thiserror::Error)]
enum MyError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

If a module boundary is crossed and errors are converted manually with `.map_err(...)`, check
whether a `From` impl would eliminate the boilerplate.

---

## Conditional Traits — Check with the Engineer Before Adding

These traits are appropriate in specific circumstances. If the condition appears to be met,
**do not add the trait automatically** — flag it and ask the engineer to confirm before proceeding.

### `Clone` — Types Passed Across Ownership Boundaries

**Add if:** The type is moved into multiple call sites, cloned explicitly in existing code, or
passed into async tasks or threads where shared ownership is needed.

**Do not add if:** The type holds large buffers, file handles, database connections, or other
resources where cloning would be expensive or semantically incorrect.

**Ask:** "Type `Foo` is used in several places where ownership is transferred. Would you like
me to derive `Clone` to make this more ergonomic, or do you prefer to keep the ownership
explicit?"

### `PartialEq` / `Eq` — Domain Types Used in Tests or Comparisons

**Add if:** The type is used in test assertions (`assert_eq!`) or compared with `==` in
application logic. For types used heavily in tests, derive these at the point of definition
rather than waiting until a test fails to compile.

**Do not add if:** The type contains floats (derive `PartialEq` only, never `Eq`) or fields
where equality is ambiguous or meaningless.

**Ask:** "Type `Foo` appears in test assertions but doesn't implement `PartialEq`. Should I
derive it?"

### `Default` — Config Structs and Builder-Style Types

**Add if:** The type is a configuration struct, an options/settings type, or has a meaningful
zero/empty state that would be used for initialization.

**Do not add if:** There is no sensible default — forced defaults on types that require real
values lead to bugs.

**Ask:** "Type `Foo` looks like a config or options struct. Would a `Default` implementation
make sense here, or do all fields require explicit values?"

### `Serialize` / `Deserialize` (serde) — Types at Serialization Boundaries

**Add if:** The type is used in any of the following:
- Reading from or writing to config files (TOML, YAML, JSON, etc.)
- API request or response bodies
- Persisted state (databases, files, caches)
- Inter-service message payloads

**Do not add if:** The type is purely internal with no serialization use case.

**Ask:** "Type `Foo` appears to cross a serialization boundary (config / API / persistence).
Should I add `serde` derives?"

---

## Traits to Add Only When the Use Case Is Explicit

Do not derive or implement these unless there is a concrete, present use case in the code being
reviewed. Do not ask — just leave them absent until needed.

| Trait | Add when |
|---|---|
| `Hash` | The type is used as a `HashMap` or `HashSet` key |
| `PartialOrd` / `Ord` | The type is sorted with `.sort()` or used in a `BTreeMap`/`BTreeSet` |
| `Display` (non-error) | The type has a meaningful user-facing string representation |
| `Copy` | The type is small, cheap, and has no ownership semantics (e.g., IDs, coordinates) |

---

## Threading and Async — Architectural Awareness

`Send` and `Sync` are auto-traits and cannot be derived, but whether a type satisfies them is
determined by its fields. Flag any of the following situations:

- A type that is passed into a `tokio::spawn` or `std::thread::spawn` closure but contains
  non-`Send` fields (e.g., `Rc`, `RefCell`, raw pointers)
- A type intended for shared state across threads that is not `Sync`

**Ask:** "Type `Foo` contains `RefCell` (or `Rc`, or a raw pointer) and appears to be used
across an async task boundary. This will not satisfy `Send`. Should we discuss the design?"

---

## Review Procedure

For each type defined in any file that appears in the diff — including types that were not
directly changed:

1. Is `Debug` derived? If not, add it.
2. Is this an error type? If so, are `Display`, `Error`, and `From` implemented? If not, add them.
3. Does any existing code clone this type manually? Does it cross task/thread boundaries?
   Consider `Clone` — ask first.
4. Is this type used in test assertions? Consider `PartialEq`/`Eq` — ask first.
5. Is this a config or options struct? Consider `Default` — ask first.
6. Does this type appear at a serialization boundary? Consider serde derives — ask first.
7. Is this type used as a map key or sorted? If so, add `Hash` or `Ord` as appropriate.
8. Does this type contain `Rc`, `RefCell`, or raw pointers and appear in async or threaded
   contexts? Flag it.
