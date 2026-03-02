# Releasing pftui

## Versioning

Semantic versioning: `vMAJOR.MINOR.PATCH`
- **v0.x.y** — pre-1.0, expect breaking changes
- Minor bump for features, patch for fixes
- Start at `v0.1.0`

## Release Process

Releases are driven by **git tags**. The feedback reviewer cron decides when to cut a release based on:
- All three beta tester scores trending upward and ≥ 7
- Significant features landed since last release
- No known P0 bugs outstanding

### To cut a release:
1. Update version in `Cargo.toml`
2. Commit: `release: vX.Y.Z`
3. Tag: `git tag vX.Y.Z`
4. Push: `git push origin master --tags`
5. GitHub Actions handles the rest automatically

## What GitHub Actions Does on Tag Push

1. **Test** — `cargo test`, `cargo clippy` on all targets
2. **Build** — Release binaries for:
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
3. **GitHub Release** — Create release with binaries + SHA256 checksums
4. **crates.io** — `cargo publish`
5. **Homebrew** — Update formula in `skylarsimoncelli/homebrew-tap`

## Secrets Required (GitHub repo settings)

- `CARGO_REGISTRY_TOKEN` — crates.io API token
- `HOMEBREW_TAP_TOKEN` — PAT with repo access to `homebrew-tap` repo
