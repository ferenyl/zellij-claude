# Releasing

Releases are created in GitHub. Publishing a release triggers
`.github/workflows/release.yml`, which builds and uploads everything.

## Steps

1. Make sure `main` is green in CI.
2. Create the release, either on GitHub under *Releases → Draft a new release*, or:

   ```sh
   gh release create v0.1.1 --target main --title v0.1.1
   ```

   - Tag: exactly `vMAJOR.MINOR.PATCH`, e.g. `v0.1.1`, created from `main`. The tag
     decides the version; `Cargo.toml` does not need to be bumped by hand.
   - Text you write in the release form is kept at the top of the notes. Use it for
     highlights or upgrade notes; the list of changes is generated.
3. Publish. The workflow then:
   - sets the crate version from the tag (fails for any other format, e.g. `v1.2`,
     `0.1.1` or `v0.1.1-rc.1`),
   - runs fmt, clippy and tests,
   - builds `claude-sessions.wasm`,
   - uploads `claude-sessions.wasm`, `claude-zellij-hook.sh`,
     `claude-sessions-<version>.tar.gz` and `SHA256SUMS` to the release,
   - adds a build provenance attestation,
   - fills in the release notes: your text, GitHub's generated list of merged pull
     requests and contributors, install instructions and checksums.
4. Check the run under *Actions* and the assets on the release page.

## If the workflow fails

Fix the problem on `main`, then either re-run the workflow from the *Actions* tab
(it runs on the tagged commit), or delete the release and tag and create them again
on the fixed commit:

```sh
gh release delete v0.1.1 --cleanup-tag --yes
```

Re-running is safe: assets are replaced and the generated part of the notes is
rewritten, not duplicated.

## Version in the repository

The workflow sets the version only for the build; it does not commit it back.
`Cargo.toml` on `main` may lag behind the latest release. Bump it in a regular PR if
you want them in sync.
