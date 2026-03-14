---
name: pftui-feature
description: Implement a pftui repo feature end-to-end from a TODO feature id such as F40 or F40.4. Use when Codex is asked to work on this repository by feature number, complete every subtask for that feature, sync the repo first, read the core product and agent docs, update code/tests/docs, and commit and push after each validated subtask.
---

# pftui Feature

Execute one `TODO.md` feature to completion. Treat the feature id passed by the user as the scope anchor.

## Inputs

- A feature id in the form `FXX` or `FXX.Y`
- The current `pftui` repository checkout

## Repo Root

Work from the active repo root. In this repo that is usually the current workspace, but the canonical files are the equivalents of:

- `PRODUCT-VISION.md`
- `PRODUCT-PHILOSOPHY.md`
- `docs/VISION.md`
- `AGENTS.md`
- `CLAUDE.md`
- `docs/ARCHITECTURE.md`
- `TODO.md`

If the user mentions `/root/pftui/...`, map those paths to the same files in the current checkout when needed.

## Workflow

### 1. Sync safely before coding

1. Check `git status --short`.
2. If the worktree is dirty with changes you did not make, do not overwrite them. Ask the user before pulling or rebasing.
3. Authenticate as the repo owner account before syncing. Use the repo's expected GitHub CLI account flow:
   - Prefer `gh auth switch -u skylarsimoncelli`
   - If the environment exposes a repo-specific wrapper named `gh auth skylarsimoncelli`, use that
4. Fetch and pull the latest changes from the current branch without discarding local work.
5. Create a dedicated feature branch before making repo changes. Use a branch name derived from the feature id, for example `feat/f40-cli-hierarchy`.

### 2. Read the alignment docs in order

Read these before planning implementation:

1. `PRODUCT-VISION.md`
2. `PRODUCT-PHILOSOPHY.md`
3. `docs/VISION.md`
4. `AGENTS.md`
5. `CLAUDE.md`
6. `docs/ARCHITECTURE.md`

Extract the constraints that matter for the target feature, especially:

- Human + agent collaboration is the product center
- Local-first and zero-config defaults are hard requirements
- Density, vim UX, privacy, and theme coherence matter for UI work
- Deep CLI hierarchy is preferred over flat namespaces
- Every CLI feature needs `--json`
- Do not touch real user financial data
- Do not modify `README.md` or `website/` unless explicitly requested

### 3. Locate the feature in `TODO.md`

1. Find the exact heading or sub-heading matching the requested feature id.
2. Read the entire feature block, not just the one line.
3. Determine the execution units:
   - If the feature already has explicit subtasks, use them
   - If the feature is broad and has no explicit checklist, split it into concrete substeps before coding
4. Keep the feature scoped to completion. Do not stop after the first subtask.

### 4. Read the relevant code before editing

Start with `docs/ARCHITECTURE.md`, then use `rg` to find the modules named by the feature. Read enough surrounding code to understand:

- CLI definitions and command dispatch
- command handlers
- models and database shape
- TUI/web entry points if the feature crosses interfaces
- tests already covering adjacent behavior

Do not guess the architecture from file names alone.

### 5. Deliver one subtask at a time

For each subtask in the feature:

1. Mark the active item in `TODO.md` as in progress if the TODO structure supports it.
2. Implement the code change.
3. Add or update tests for any behavior or logic changes.
4. Update repo documentation where appropriate so the feature aligns with the product:
   - `TODO.md`
   - `CHANGELOG.md`
   - command docs/help text
   - user/operator docs only when the behavior actually changed
5. Run validation:
   - `cargo fmt` if Rust files changed
   - targeted tests during iteration
   - `cargo test`
   - `cargo clippy -- -D warnings` unless the repo currently uses a different established clippy invocation
6. Only commit when checks pass.
7. Commit with the required author identity:

```bash
GIT_COMMITTER_NAME="skylarsimoncelli" \
GIT_COMMITTER_EMAIL="skylar.simoncelli@icloud.com" \
git commit --author="skylarsimoncelli <skylar.simoncelli@icloud.com>" -m "<clear subtask message>"
```

8. Push immediately after each successful subtask commit.

Repeat until the entire requested feature is done.

### 6. Finish the feature cleanly

Before stopping:

1. Ensure every subtask under the requested feature is complete.
2. Mark finished TODO items as done.
3. Add the final changelog entry or entries needed to explain shipped behavior.
4. Re-run the final validation suite after the last change.
5. Push the final branch state.
6. Open or update a pull request from the feature branch to the base branch.
7. Merge through the pull request path after validation is green. Do not push feature commits directly to `master`.

## Output Standard

When using this skill, the work product should leave the repo in a state where:

- the requested `FXX` feature is complete, not partial
- each completed subtask has its own validated commit and push
- tests and lint pass before every commit
- documentation matches the shipped behavior
- the implementation still aligns with product vision and philosophy
- the work lands through a feature branch and PR merge, not direct pushes to `master`

## Guardrails

- Never expose or use real portfolio data from local pftui databases.
- Never use destructive git commands to force sync over user changes.
- Never skip tests or clippy before committing.
- Never leave `TODO.md` showing in-progress work without a matching follow-up commit.
- Never work directly on `master`; always branch first and merge via PR.
- Prefer repo conventions over generic solutions when they differ.
