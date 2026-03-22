# nano-os

@.claude/process/planning-workflow.md
@.claude/process/development-workflow.md
@.claude/process/git-workflow.md

## Project

RISC-V kernel targeting QEMU virt (development) and LicheeRV Nano / SG2002 (hardware).

## Before Writing Code

- Search before creating any new type or function — check whether an equivalent already exists
- Sketch the call site before fixing the signature — awkward call sites signal a poor API

## Design Reference

Linux is the primary design reference. Consult it for design decisions; document
deviations and the reason for them.

## Backlog

Deferred or out-of-scope ideas go to `backlog/`, one file per topic.
