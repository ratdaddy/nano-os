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
- **Implementation**: High-level approach (not full code)
- **Next Steps**: Ordered action items

**Anti-pattern**: Don't write the full implementation in the plan - it becomes the codebase.

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

## Working Together

### Sign-Off Required

Don't start implementation until:
- Plan is reviewed together
- Approach is approved
- First slice is identified

This prevents "wrong direction" refactors.

### Implementing Steps

**CRITICAL: Only implement what is explicitly requested.**

When instructed to "implement Step 2.3" or "implement the file_type() function":
- ✅ Do ONLY that step/substep
- ❌ Do NOT implement related steps
- ❌ Do NOT implement "while we're here" improvements
- ❌ Do NOT jump ahead to the next step

**Wait for explicit instruction** before moving to the next step.

This prevents:
- Scope creep
- Implementing things in wrong order
- Building features that may change after learning from current step

### When Plans Diverge

If implementation reveals issues:
- Stop and discuss
- Update or replace the plan
- Don't push ahead on wrong path

### Small Iterations

Better: Small plan → implement → learn → next small plan
Worse: Mega-plan covering 6 phases before any implementation

---

## Reference Documents

These live in `notes/` and define standards:

**Required**:
- `notes/filesystem-naming.md` - Naming conventions for all filesystem code
- `notes/ext2-ondisk-format.md` - ext2 specification reference

**Follow these religiously** - they prevent inconsistency and rework.

---

## File Organization

```
plans/
  ROADMAP.md           ← Master index (keep updated)
  active-plan-1.md     ← Currently working on
  active-plan-2.md     ← Currently working on
  completed/           ← Archive completed plans here

notes/
  filesystem-naming.md ← Standards (required reading)
  ext2-ondisk-format.md
  planning-workflow.md ← This file
```

**Keep `plans/` clean** - only active plans + ROADMAP.md

---

## Key Principles

1. **Vertical slices** - Build complete features, not layers
2. **One plan per area** - Consolidate overlapping plans
3. **Sign-off first** - Review before coding
4. **Update ROADMAP** - When plans change state
5. **Follow standards** - Check notes/ for conventions
6. **Keep it short** - Plans describe WHAT, not HOW

---

Last updated: 2026-02-20
