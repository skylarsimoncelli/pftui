# Distribution Readiness

This document captures what remains for non-F32 TODO distribution items and what is already prepared in-repo.

## Current State

`TODO.md` marks these as blocked:
- Snap/AUR/Scoop publishing (requires external publisher accounts + CI secrets)
- Homebrew Core submission (requires Homebrew inclusion prerequisites)

The repository is now prepared to reduce operational friction once blockers clear.

## In-Repo Readiness Added

- `scripts/check_distribution_versions.sh`
  - Validates `Formula/pftui.rb`, `snap/snapcraft.yaml`, and `scoop/pftui.json` versions match `Cargo.toml`.
- `scripts/update_distribution_manifests.sh`
  - Updates all three manifests for a new release version and checksums.
- CI gate in `.github/workflows/ci.yml`
  - Runs distribution version consistency checks on PR/push.

## Required External Inputs

### Snap

Prereqs:
- Snapcraft publisher account
- Store registration for package name `pftui`

Suggested secrets:
- `SNAPCRAFT_STORE_CREDENTIALS`

### Scoop

Prereqs:
- Access to Scoop bucket (or own bucket repo)
- Ability to push updated `pftui.json`

Suggested secrets:
- `SCOOP_BUCKET_TOKEN`

### AUR

Prereqs:
- AUR account + SSH key
- `pftui` package namespace available

Suggested secrets:
- `AUR_SSH_PRIVATE_KEY`

### Homebrew Core

Prereqs:
- Homebrew/core acceptance threshold and policy compliance
- Formula audit pass

Note:
- Until core acceptance is possible, continue maintaining `skylarsimoncelli/homebrew-tap`.

## Release-Day Checklist

1. Tag release and publish GitHub artifacts.
2. Compute checksums for:
   - `pftui-<version>.tar.gz`
   - `pftui-x86_64-windows.exe`
3. Run:
   - `scripts/update_distribution_manifests.sh <version> <tar_sha256> <win_sha256>`
4. Validate:
   - `scripts/check_distribution_versions.sh`
5. Commit manifest updates.
6. Publish to external stores once credentials/accounts are available.

## Notes

These steps intentionally avoid changing runtime behavior. They only reduce the manual work required when external blockers are removed.
