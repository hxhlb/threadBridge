# Releases

This directory contains:

- per-version release notes, for example `docs/releases/0.1.0-rc.1.md`
- the repo release runbook at [repo-release-runbook.md](/Volumes/Data/Github/threadBridge/docs/releases/repo-release-runbook.md)

Use the runbook when you need to publish a new macOS prerelease from this repo:

```bash
scripts/release_threadbridge.sh release \
  --version 0.1.0-rc.1 \
  --notes-file docs/releases/0.1.0-rc.1.md \
  --codesign-identity "Developer ID Application: Example, Inc. (TEAMID)"
```

Current committed contract:

- `release_threadbridge.sh` handles build, sign, DMG, notarize, and GitHub draft prerelease upload
- git tag creation and final draft publication are separate maintainer steps
- Homebrew tap publication is still out of scope for the first RC path
