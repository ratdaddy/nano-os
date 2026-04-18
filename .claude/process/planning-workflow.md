# Planning Workflow

How we create and manage plans for nano-os development.

---

## The Master Index

**`plans/ROADMAP.md`** is the single source of truth for:
- What's currently being worked on
- What's planned next
- What's recently completed
- Key reference documents

Update it when plans change state.

---

## Creating New Plans

### Before Writing Code

1. **Identify the goal** - One sentence describing the end state
2. **Check for existing plans** - Consolidate if overlap exists
3. **Determine the approach** - Vertical slices preferred (build end-to-end feature, then expand)
4. **Get sign-off** - Review plan together before implementation starts

### Plan Structure

Keep plans **short and focused**:
- **Goal**: What we're building (one sentence)
- **Why**: Problem being solved
- **Prerequisites**: What must exist first
- **Deliverables**: Concrete outcomes
- **Implementation**: Ordered steps; each step includes what to verify when done

**Anti-pattern**: Don't write the full implementation in the plan - it becomes the codebase.

**No "Next Steps" section**: Steps belong in Implementation. A "Next Steps" section that recaps the implementation steps is redundant and should not be added.

**Verification in steps, not gate names**: Each step's verification note should state what to actually check or observe (e.g., "cross-compile clean, unit tests pass" or "boot to shell, confirm X appears in output"). Do not refer to verification by gate number (Gate 1, Gate 2, Gate 3) — those labels are internal workflow machinery, not meaningful to a plan reader.

**Step sequencing**: Order steps so each one delivers an observable, working piece
of functionality before the next begins. For a feature with multiple subcomponents,
develop each subcomponent end-to-end — enough to verify it works — before moving
to the next. This keeps the system in a testable state throughout and surfaces
integration problems early rather than at the end.

---

## Plan Lifecycle

```
Draft → Review → Active → Complete → Archive
```

1. **Draft** - Exploring the approach
2. **Review** - Discuss and approve together
3. **Active** - Currently implementing (listed in ROADMAP.md)
4. **Complete** - Finished, moved to plans/completed/
5. **Archive** - Old plans in completed/ directory

**Rule**: Only ONE active plan per major area (filesystem, block layer, etc.)

---

## Re-Planning

If implementation reveals that a plan needs to change, the development workflow
will trigger a return to planning. At that point:

1. Stop implementation
2. Assess whether the current plan can be amended or needs to be replaced
3. Update or replace the plan following the same structure and lifecycle as above
4. Get approval before resuming implementation

Re-plans should be small — fix the specific thing that was wrong, not a broader
redesign unless that's what the situation requires.

## Planning Principles

- Prefer small plans: small plan → implement → learn → next small plan
- Avoid mega-plans covering many phases before any implementation
- Sequence steps for observability — each step should leave the system in a
  state where the new functionality can be seen working

---

## Reference Documents

`ref/` contains technical standards and specifications that apply to the
codebase — naming conventions, format specs, hardware references, coding
standards. Before starting a plan, check whether any relevant reference
documents exist and consult them. Following established standards prevents
inconsistency and rework.

---

## File Organization

```
plans/
  ROADMAP.md           ← Master index (keep updated)
  active-plan-1.md     ← Currently working on
  active-plan-2.md     ← Currently working on
  completed/           ← Archive completed plans here

ref/
  *.md                 ← Technical standards and specifications

.claude/process/
  planning-workflow.md ← This file
```

**Keep `plans/` clean** - only active plans + ROADMAP.md

---

## Key Principles

1. **Observable slices** - Each step should deliver a piece of functionality that can be seen working before the next step starts
2. **One plan per area** - Consolidate overlapping plans
3. **Update ROADMAP** - When plans change state
4. **Follow standards** - Check ref/ for conventions
5. **Keep it short** - Plans describe WHAT, not HOW

