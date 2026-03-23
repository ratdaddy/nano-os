# Pre-Commit Check

Review the staged changes before committing.

## Step 1: Identify what changed

Run `git diff --staged` and note:
- Which files were modified or added
- Which types (structs, enums, traits) are defined in those files — including types that were not directly changed

## Step 2: Run the gates

```
make build
make test
```

Report any failures immediately. Do not proceed to the remaining steps if the build or tests fail.

## Step 3: Trait review

Apply the Review Procedure from `ref/rust-trait-checklist.md` to every type in every touched file.

## Step 4: Code style checklist

Work through the Code Review Checklist at the bottom of `ref/coding-style.md`.

## Step 5: Test coverage

For each new function or type added in the diff, ask:
- Does it contain branching logic, error handling, or parsing? If so, are there tests?
- Do the tests assert specific error variants, not just that an error occurred? (See `ref/testing-strategy.md`)
- If the function depends on a downstream implementation for its error behavior, does it use a mock rather than the real implementation?

## Step 6: Plans and ROADMAP

- Does the diff complete a step in the active plan? If so, flag it so the developer can mark it done.
- Should `plans/ROADMAP.md` be updated to reflect what was accomplished?

## Step 7: Report

Report results in three groups:

**Passes** — items confirmed clean
**Needs attention** — concrete issues to fix before committing
**Judgment calls** — things that may warrant action; flag and ask
