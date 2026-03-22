# Development Workflow

## Reference Documents
- Testing philosophy and coverage decisions: `ref/testing-strategy.md`
- Build system: `Makefile` — all verification commands run through `make`

## The Development Loop

Each step in a plan follows this cycle:

1. **Identify the step** — take the next step from the active plan
2. **Propose and get approval** — Claude describes the intended approach before writing any code; the developer approves before implementation begins
3. **Implement** — Claude writes the code, within the scope rules below
4. **Verify** — run the gates; all applicable gates must pass before the step is considered done
5. **Repeat** — a plan step may require several iterations of steps 2–4 as the implementation is decomposed into smaller pieces
6. **Confirm complete** — Claude asks the developer whether the step is complete before marking it so in the plan
7. **Advance** — update the plan, then wait for explicit instruction to begin the next step

Gate 3 applies when the step touches boot-level behavior, device initialization,
or anything not reachable by unit tests. Gates 1 and 2 apply to every step.

If implementation reveals that the plan is wrong or incomplete, exit the loop.
See `.claude/process/planning-workflow.md` — Re-Planning.

## Scope Rules

Only implement what is explicitly approved for the current step:

- Do ONLY that step — not related steps, not the next step
- Do not make "while we're here" changes without authorization

If something noticed during implementation looks worth doing — a small cleanup,
an obvious fix, something that aligns with an established standard — surface it
as a suggestion at a natural pause point. The developer decides whether to fold
it in, add it to the backlog, or skip it.

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

See `ref/testing-strategy.md` for what is and isn't covered and why.

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

Build and test output is captured to a shared log file so it's independently
visible in a terminal pane:

- Build and test: `/tmp/nano-os.log`
- QEMU session: `/tmp/nano-os-qemu.log` (captured automatically by `make run`)

Run `make tail-log` in a dedicated pane to follow the build/test log. `tail -F` is
used so the pane can be started before the log file exists.

The QEMU runner (`.cargo/config.toml`) uses `-nographic`, routing serial to stdio.
`make run` wraps the invocation with `script -q` to capture the full session without
disrupting interactivity. A QEMU-native pipe is not used as it would break the
interactive shell. Claude reads `/tmp/nano-os-qemu.log` after the session ends.

The `UserPromptSubmit` hook checks whether the log has been updated since the
last prompt. If so, the new content is injected as context at the start of the
next exchange. This means: run `make build` or `make test` in your terminal,
then type your next message — Claude will have the output without you pasting it.

## What Claude Does at Each Gate

**Pre-check (Gates 1 and 2):** Before invoking `make build` or `make test`, Claude
checks whether a pane titled "log" exists in the current tmux session:
```
tmux list-panes -a -F '#{pane_title}' | grep -q '^log$'
```
If no such pane is found, Claude asks the developer whether to proceed without the
log pane visible, or to start `make tail-log` in a pane first. Claude does not start
the pane itself.

**Gate 1 (build):** Claude invokes `make build`. Output is captured to the log
automatically. Claude reads `/tmp/nano-os.log` and reports:
```
Build: [PASS / FAIL]
Errors: <count> — <list>
Warnings: <count> — <assessment>
Log: /tmp/nano-os.log
```
Claude does not proceed to the next implementation step if the build fails.

**Gate 2 (tests):** Claude invokes `make test`. Output is captured to the log
automatically. Claude reads `/tmp/nano-os.log` and reports:
```
Tests: [PASS / FAIL]
Passed: <n> / Failed: <n>
Failed tests: <list if any>
Failure type: [boot failure / test logic failure]
Log: /tmp/nano-os.log
```
A test regression in code unrelated to the current step is noted and flagged,
not silently accepted. Claude does not proceed if tests that were passing are
now failing.

**Gate 3 (boot):** Claude does not run `make run`. After a step that affects
boot-level behavior, Claude specifies:
- Which gate 3 verification applies
- What to look for in the output
- What a passing result looks like

Claude then starts `.claude/wait-qemu.py` in the background using `/usr/bin/python3`
(not `python3`, which is unmanaged in this project):
```
/usr/bin/python3 .claude/wait-qemu.py
```
The script watches `/tmp/nano-os-qemu.log` for a new `QEMU: Terminated` line written
after the script started, waking Claude when the session ends. Claude then reads the
log and reports the result. Claude does not proceed until the result is confirmed.

## New Code and Test Coverage

When implementing a function that falls within the testable boundary defined in
`ref/testing-strategy.md` — branching logic, error paths, parsing, data
accumulation — Claude proposes the corresponding test before writing
implementation. The test is written first, confirmed failing, then implementation
follows.

For code outside that boundary, Claude documents the verification approach in a
comment block at the definition site: what correctness means for this code and
how it would be observed.

## Intentional Coverage Gaps

Not all code is within the unit test boundary. When a decision is made not to
test a module or subsystem, that decision must be recorded — either in a comment
block at the definition site explaining what correct behavior looks like and how
it would be observed, or in the relevant design document. Undocumented gaps are
not acceptable.

See `ref/testing-strategy.md` for the framework that determines
whether code falls inside or outside the testable boundary.

## Design-Level Verification

Invariants that cannot be tested at runtime — allocator safety boundaries,
interrupt safety contracts, ownership rules across unsafe blocks — are verified
through design discussion before implementation. The outcome is documented in a
comment block at the definition site. The reasoning is captured in the relevant `design/` document.

This is a legitimate verification activity. A well-reasoned comment block on an
unsafe boundary is not a placeholder for a missing test — it is the appropriate
artifact for that class of invariant.
