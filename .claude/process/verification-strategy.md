# Verification Strategy

## Reference Documents
- Testing philosophy and coverage decisions: `notes/testing-strategy.md`  
- Build system: `Makefile` — all verification commands run through `make`

## The Three Gates

### Gate 1: Build
```
make build
```
Cross-compiles the kernel for `riscv64gc-unknown-none-elf`. A clean build with 
no errors is required before anything else. Warnings are read and assessed — 
not automatically fixed, but not silently ignored either.

### Gate 2: Unit Tests
```
make test
```
Runs the unit test suite inside QEMU using `custom_test_frameworks`. This gate 
is dual-signal: the kernel must initialize far enough to execute tests before 
any test logic runs, so a `make test` failure can mean either a boot regression 
or a test logic failure. The two failure modes look different in the output — 
a boot failure stops before any test output appears.

Current coverage: ~77 tests across allocator, VFS, ramfs, procfs, initramfs, 
and dev layer. See `notes/testing-strategy.md` for what is and isn't covered 
and why.

The unit test boundary is drawn at hardware interfaces and OS primitives where 
correct behavior requires either full emulation or actual execution. Code on 
the far side of that boundary (drivers, syscall layer, page mapper, ext2) is 
verified through Gate 3.

### Gate 3: Boot and Integration
```
make run
```
Boots the kernel to the interactive shell in QEMU. Successful completion means: 
kernel initializes, allocator is live, VFS mounts, shell prompt appears. The 
interactive menu allows manual inspection of subsystem state.

Gate 3 is run by the developer, not Claude. After completing an implementation 
step, Claude specifies which inspection steps are relevant and what to look for. 
The developer runs them and reports results back before the next step proceeds.

## Log Capture and Visibility

Build and test output is captured to known log files so it's independently 
visible in a terminal pane:

- Build: `/tmp/nano-os-build.log`
- Test: `/tmp/nano-os-test.log`
- QEMU session: `/tmp/nano-os-qemu.log` (via `script -f` wrapper)

The `UserPromptSubmit` hook checks whether any of these logs have been updated 
since the last prompt. If so, the new content is injected as context at the 
start of the next exchange. This means: run `make build` or `make test` in your 
terminal, then type your next message — Claude will have the output without you 
pasting it.

## What Claude Does at Each Gate

**Gate 1 (build):** Claude can invoke `make build` using the tee pattern. It 
reads `/tmp/nano-os-build.log` and reports:
```
Build: [PASS / FAIL]
Errors: <count> — <list>
Warnings: <count> — <assessment>
Log: /tmp/nano-os-build.log
```
Claude does not proceed to the next implementation step if the build fails.

**Gate 2 (tests):** Claude can invoke `make test` using the tee pattern. It 
reads `/tmp/nano-os-test.log` and reports:
```
Tests: [PASS / FAIL]
Passed: <n> / Failed: <n>
Failed tests: <list if any>
Failure type: [boot failure / test logic failure]
Log: /tmp/nano-os-test.log
```
A test regression in code unrelated to the current step is noted and flagged, 
not silently accepted. Claude does not proceed if tests that were passing are 
now failing.

**Gate 3 (boot):** Claude does not run `make run`. After a step that affects 
boot-level behavior, Claude specifies:
- Which gate 3 verification applies
- What to look for in the output
- What a passing result looks like

The developer runs and reports. Claude does not proceed until the result is 
confirmed.

## New Code and Test Coverage

When implementing a function that falls within the testable boundary defined in 
`notes/testing-strategy.md` — branching logic, error paths, parsing, data 
accumulation — Claude proposes the corresponding test before writing 
implementation. The test is written first, confirmed failing, then implementation 
follows.

For code outside that boundary, Claude documents the verification approach in a 
comment block at the definition site: what correctness means for this code and 
how it would be observed.

## Intentional Coverage Gaps

The following areas have no unit tests by design:
- Hardware drivers (UART, SD, VirtIO) — hardware-coupled
- ext2 filesystem — format-sensitive, in active development
- Syscall layer, process/thread management, page mapper — system-level primitives

These gaps are not tracked as deficiencies. ext2 coverage will be revisited when 
the implementation stabilizes.

## Design-Level Verification

Invariants that cannot be tested at runtime — allocator safety boundaries, 
interrupt safety contracts, ownership rules across unsafe blocks — are verified 
through design discussion before implementation. The outcome is documented in a 
comment block at the definition site. The reasoning is captured in a `notes/` 
document for the relevant subsystem.

This is a legitimate verification activity. A well-reasoned comment block on an 
unsafe boundary is not a placeholder for a missing test — it is the appropriate 
artifact for that class of invariant.
