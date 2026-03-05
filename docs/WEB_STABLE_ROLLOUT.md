# Web Stable Rollout Sequence

This document defines the release order for publishing stable `pftui web`.

## Preconditions
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo test --all-features` passes.
- Playwright integration and visual suites pass (`npm run test:web`).
- Parity checklist gate passes for required items:
  `22,25,26,27,28,29,30,34,37,38,40,41,42,43,44,45,46,47,49,50,51`.

## Tagging Rule
- Use tags prefixed with `web-stable-` for stable web releases.
- Release workflow enforces the parity checklist gate for `web-stable-*` tags.

## Rollout Steps
1. Merge all web-hardening work to `master`.
2. Wait for CI (`CI` workflow) to pass including web integration/visual jobs.
3. Create and push a `web-stable-*` tag.
4. Verify release workflow `test` job passes:
   Rust tests, Playwright suites, and parity checklist gate.
5. Verify artifacts:
   binaries/packages and uploaded visual snapshot artifacts.
6. Publish release notes with checklist evidence references.
