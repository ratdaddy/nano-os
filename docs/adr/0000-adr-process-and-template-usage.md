# 0000: ADR Process and Template Usage

- **Status**: Adopted
- **Date**: 2026-05-29
- **Author**: Brian VanLoo

## Context

Architecture Decision Records (ADRs) are structured, version-controlled documents that
capture **why** a decision was made, **what** was decided, and **its expected consequences**.
They serve as a durable knowledge base for both human maintainers and AI coding agents,
ensuring that the reasoning behind key choices remains accessible over the life of the project.

For AI agents, ADRs enable:
- Understanding of the rationale and constraints behind decisions
- Avoidance of changes that contradict established design principles
- Identification of when a decision should be revisited or superseded
- Quick navigation to related ADRs and relevant code sections

For humans, ADRs reduce the need to reconstruct decision history from commit logs or memory,
especially when trade-offs or subtle constraints influenced the outcome.

nano-os has particular need for ADRs because:
- Systems-level decisions have long-lasting consequences; the cost of reversing them is high
- Linux is the primary design reference; deviations from Linux conventions need explicit rationale
- Hardware constraints (RISC-V, T-Head C906 quirks, LicheeRV Nano specifics) produce
  non-obvious decisions that are easy to violate without documented context
- Safety invariants around `unsafe` code must be reasoned about, not just tested

## Decision

This project will maintain ADRs in the `docs/adr/` directory, numbered sequentially
starting at `0000`.

### ADR Template

```markdown
# [ADR Number]: [Short Descriptive Title]

- **Status**: [Adopted | Superseded | Deprecated | Considering | Future]
- **Date**: YYYY-MM-DD
- **Author**: [Name(s)]

## Context

[Background, problem statement, constraints, assumptions, and trade-offs considered.]

## Decision

[The chosen solution, or proposed solution if not yet adopted.]

## Consequences

[Anticipated positive and negative outcomes, including impact on maintainability,
safety, performance, and AI agent reasoning.]

## Alternatives Considered

[Other viable options evaluated, with the reasons they were not selected.]

## References

[Links to related ADRs, design docs in docs/design/, source files, or external specs.]
```

### When to Create a New ADR

Create an ADR when any of the following conditions are met:

1. **A choice between alternatives** — a design discussion concludes by selecting one
   approach over others. The alternatives considered and the reasons for rejection are
   worth preserving.

2. **A deviation from the Linux reference** — the Linux design is the default; any
   deliberate departure requires documented rationale. See ADR 0001.

3. **A new cross-cutting abstraction** — a new trait, interface, or module boundary is
   established that other subsystems will depend on. The contract and its constraints
   belong in an ADR.

4. **A constraint identified that affects future work** — a hardware limitation, safety
   invariant, or compatibility requirement that will shape decisions not yet made.

5. **A non-obvious approach adopted for a specific reason** — a workaround, a pattern
   that looks wrong but is correct, or code that will confuse a future reader without
   the context that led to it.

6. **An explicit exclusion decision** — a decision made NOT to implement something,
   and why. These are as important as inclusion decisions; they prevent re-evaluation
   of already-discarded ideas.

### Index Maintenance

`docs/adr/README.md` is the index for all ADRs and must be kept up to date whenever an
ADR is created, adopted, superseded, deprecated, or changes status.

The index must:
- Include clickable links to each ADR
- Track three sections: **Active ADRs** (Adopted), **Considering**, **Future**
- Track **Implementation** state for Active ADRs: **Complete**, **Partial**, or **None**

### Maintenance Rules

- **Version control**: All ADRs are plain Markdown files tracked in Git
- **Immutability**: Adopted ADRs are not altered except for clarifications or typo fixes;
  changes to a decision require a new ADR that supersedes the old one
- **Linking**: ADRs should cross-reference related decisions and affected code or design docs
- **AI readability**: Use explicit, consistent wording; avoid jargon; clearly describe
  constraints, dependencies, and rejected alternatives

## Consequences

- Creates a decision log that supports human and AI contributors
- Reduces risk of architectural drift or contradictory changes
- Provides AI agents with reliable, structured context for automated reasoning
- Makes it clear why certain paths were *not* taken, preventing re-evaluation of discarded ideas
- Requires discipline: an undocumented decision that should have been an ADR is a gap

## Alternatives Considered

- **Relying solely on design docs** (`docs/design/`) for decision context
  - *Rejected because*: Design docs describe current state; they don't record the alternatives
    considered or why a different path was not taken. Both are needed.
- **Relying solely on commit messages**
  - *Rejected because*: Commit messages are too granular and inconsistent to reconstruct
    architectural reasoning reliably.
- **No formal decision records**
  - *Rejected because*: nano-os has a large surface area of non-obvious decisions —
    hardware constraints, unsafe invariants, Linux deviations — that will be repeatedly
    re-derived or violated without a durable record.

## References

- [ADR 0001](0001-foundational-architecture.md) — foundational architecture baseline
- `docs/design/` — architecture state documents (what was built; ADRs record why)
- [Michael Nygard's ADR format](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions.html)
