# Git Workflow

## Commits

The developer makes all commits and writes all commit messages. Claude does not
stage files or run `git commit` unless explicitly asked to for a specific commit.

When preparing a commit, do not automatically stage files from `plans/` — those
are committed separately from code changes.

## Moving Files

Always use `git mv` rather than `mv` + `git add` when moving tracked files.
This preserves history and makes the rename visible as a rename in the diff
rather than a delete + add.

If a file is untracked, plain `mv` is required — `git mv` will fail. In that
case use `mv` then `git add` on the destination.
