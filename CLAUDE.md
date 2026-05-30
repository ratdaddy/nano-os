# nano-os

@~/.claude/process/planning-workflow.md
@.claude/process/development-workflow.md

## Project

RISC-V kernel targeting QEMU virt (development) and LicheeRV Nano / SG2002 (hardware).

## Before Writing Code

- Search before creating any new type or function — check whether an equivalent already exists
- Sketch the call site before fixing the signature — awkward call sites signal a poor API

## Design Reference

Linux is the primary design reference. Consult it for design decisions; document
deviations and the reason for them.

## Documentation

- **Architecture decisions** — `docs/adr/`; consult before making significant design choices.
  Watch for ADR trigger conditions (see `docs/adr/0000`) during design discussions — when
  one is met, complete the discussion first, then flag it as a candidate before moving on.
- **Design references** — `docs/design/`; system structure, interfaces, hardware constraints
- **Technical standards** — `docs/ref/`; coding style, naming conventions, hardware specs
- **Planning** — `docs/initiatives/`; active initiatives, plans, and ROADMAP

## Backlog

Deferred or out-of-scope ideas go to `backlog/`, one file per topic.
