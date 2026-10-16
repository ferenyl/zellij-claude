# Releasing

Releases are created in GitHub. Publishing a release triggers
`.github/workflows/release.yml`, which builds and uploads everything.

## Steps

1. Make sure `main` is green in CI.
2. In `CHANGELOG.md`, rename `## [Unreleased]` to the new version and date, e.g.
   `## [1.2.0] - 2026-11-01`, and add a fresh empty `## [Unreleased]` above it.
   Commit and push to `main`.
3. Create the release, either on GitHub under *Releases → Draft a new release*, or:

   ```sh
   gh release create v1.2.0 --target main --title v1.2.0
   ```

   - Tag: `vMAJOR.MINOR.PATCH`, created from `main`. The tag decides the version;
     `Cargo.toml` does not need to be bumped by hand.
   - Pre-release: use a tag like `v1.2.0-rc.1` and tick *Set as a pre-release*
     (`--prerelease` with `gh`).
   - Text you write in the release form is kept at the top of the notes.
4. Publish. The workflow then:
   - sets the crate version from the tag (fails if it is not semantic, e.g. `v1.2`),
   - runs fmt, clippy and tests,
   - builds `claude-sessions.wasm`,
   - uploads `claude-sessions.wasm`, `claude-zellij-hook.sh`,
     `claude-sessions-<version>.tar.gz` and `SHA256SUMS` to the release,
   - adds a build provenance attestation,
   - fills in the release notes: your text, the matching `CHANGELOG.md` section,
     GitHub's generated list of merged pull requests and contributors, install
     instructions and checksums.
5. Check the run under *Actions* and the assets on the release page.

## If the workflow fails

Fix the problem on `main`, then either re-run the workflow from the *Actions* tab
(it runs on the tagged commit), or delete the release and tag and create them again
on the fixed commit:

```sh
gh release delete v1.2.0 --cleanup-tag --yes
```

Re-running is safe: assets are replaced and the generated part of the notes is
rewritten, not duplicated.

## Version in the repository

The workflow sets the version only for the build; it does not commit it back.
`Cargo.toml` on `main` may lag behind the latest release. Bump it in a regular PR if
you want them in sync.
