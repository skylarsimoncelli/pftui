# CONTRIBUTING.md — Repository Development Rules

> Read this before making any code changes. These rules apply to all contributors: human developers, AI coding agents, and automated cron jobs.

## Branch Protection

**The `master` branch is protected.** All changes must go through pull requests.

- Direct pushes to `master` are blocked
- PRs require passing CI (tests + clippy) before merge
- Force pushes to `master` are prohibited

## Development Workflow

### 1. Use Git Worktrees (recommended)

Git worktrees let you work on a feature branch without disturbing your main checkout. This is the preferred workflow for all development:

```bash
cd /root/pftui

# Create a worktree with a new branch off master
git fetch origin
BRANCH="dev/$(date +%Y%m%d-%H%M)-short-description"
git worktree add /tmp/pftui-work -b "$BRANCH" origin/master

# Work in the worktree
cd /tmp/pftui-work

# ... make changes, test, commit ...

# Push and create PR
git push origin "$BRANCH"
gh pr create --base master --head "$BRANCH" \
  --title "Short descriptive title" \
  --body "What this PR does and why."

# Merge (if tests pass)
gh pr merge "$BRANCH" --squash --delete-branch

# Cleanup
cd /root/pftui
git worktree remove /tmp/pftui-work
git fetch origin && git pull
```

### 2. Branch Naming

Use descriptive prefixes:
- `dev/YYYYMMDD-HHMM-feature-name` — new features or TODO items
- `fix/YYYYMMDD-short-description` — bug fixes
- `docs/short-description` — documentation only

### 3. Pre-Commit Checks

Before pushing, always run:

```bash
source "$HOME/.cargo/env"
cargo test                                    # all tests must pass
cargo clippy --all-targets -- -D warnings     # no warnings
```

Both must pass. Do not open a PR with failing tests or clippy warnings.

### 4. Commit Messages

Write clear, descriptive commit messages:

```
Fix TIMESTAMPTZ decode crash in technical_snapshots

The Rust struct used String for computed_at but the Postgres column
is TIMESTAMPTZ. Added ::text cast in SELECT query to match the
existing fix pattern used in other tables.

Closes TODO: P0 technical_snapshots crash
```

One focused commit per PR. If a feature has multiple logical steps, squash on merge.

### 5. PR Description

Include:
- What the PR does
- Which TODO item it addresses (if applicable)
- Test results (`cargo test` output summary)
- Any breaking changes or migration notes

### 6. Post-Merge

After merging a PR:
- Remove completed items from `TODO.md`
- Update `CHANGELOG.md` with what shipped
- Commit these updates directly to master (documentation-only commits are exempt from PR requirement)

## For AI Agents

All AI coding agents (dev-agent cron, spawned sub-agents, manual agent runs) must follow this workflow. Key points:

1. **Never commit directly to master.** Always use a branch + PR.
2. **Use git worktrees** to avoid conflicts with the main checkout.
3. **Run tests and clippy** before pushing. Both must pass.
4. **Clean up worktrees** after merging.
5. **Git author:** `skylarsimoncelli <skylar.simoncelli@icloud.com>` (unless configured otherwise)

Read [CLAUDE.md](CLAUDE.md) for code standards, CLI design rules, and architecture reference before making changes.

## What Not to Change

- `README.md` and `website/` are locked. Do not modify without explicit maintainer approval.
- Do not commit real financial data, API keys, or personal information.
- Do not add dependencies without clear justification.
