# Git Workflow

## Commits

The developer makes all commits and writes all commit messages. Claude does not
stage files or run `git commit` unless explicitly asked to for a specific commit.

When preparing a commit, do not automatically stage files from `plans/` — those
are committed separately from code changes.

## Moving Files

Before moving any file, check whether it is tracked:
```
git ls-files --error-unmatch <file>
```
Exit code 0 means tracked; non-zero means untracked.

Use `git mv` for tracked files — preserves history and shows the rename as a
rename in the diff rather than a delete + add.

Use plain `mv` for untracked files — `git mv` will fail on them. No `git add`
needed on the destination unless the file should be staged.
