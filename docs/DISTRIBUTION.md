# Distribution Playbook

This document covers package publishing targets that depend on external accounts and secrets:

- Snap Store
- AUR
- Scoop bucket
- Homebrew Core

## Current State

- In-repo packaging files exist: `snap/snapcraft.yaml`, `scoop/pftui.json`.
- Manifest automation scripts exist:
  - `scripts/prepare_distribution_manifests.sh`
  - `scripts/render_aur_pkgbuild.sh`
  - `scripts/update_scoop_manifest.sh`
- Final publish is still blocked on external maintainer credentials and store approvals.

## Prepare Manifests From Release Artifacts

After a GitHub release, collect artifacts into `./release` and run:

```bash
scripts/prepare_distribution_manifests.sh 0.1.0 ./release
```

This updates:

- `packaging/aur/PKGBUILD` using Linux artifact SHA256
- `scoop/pftui.json` using Windows artifact SHA256

## Snap Publishing

Prerequisites:

- Snapcraft account with publisher access for `pftui`
- Store credentials exported for CI/manual use

Manual publish flow:

```bash
snapcraft
snapcraft upload --release=stable pftui_*.snap
```

## AUR Publishing

Prerequisites:

- AUR maintainer account
- SSH key with push access to `aur@aur.archlinux.org:pftui.git`

Flow:

1. Run manifest prep script to regenerate `packaging/aur/PKGBUILD`.
2. Clone AUR repo.
3. Copy `PKGBUILD`, generate `.SRCINFO`, commit, push.

## Scoop Publishing

Prerequisites:

- Maintainer access to target Scoop bucket repository
- GitHub token with repo write scope

Flow:

1. Run manifest prep script to refresh `scoop/pftui.json`.
2. Open PR against Scoop bucket with updated manifest.

## Homebrew Core

Homebrew Core submission is currently blocked by Homebrew eligibility requirements (project popularity threshold and review acceptance). Continue publishing to tap:

- `skylarsimoncelli/homebrew-tap`
