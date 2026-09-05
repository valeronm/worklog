# Cutting a release

A release is a tag. Pushing `vX.Y.Z` runs the checks, builds the binary for
Linux x86_64 and aarch64 and macOS arm64 on their own runners, and creates
or updates the GitHub release with each binary and its `.sha256`.
`packaging/get.sh` resolves the latest release from the redirect and
installs it.

1. Bump `version` in `Cargo.toml` and commit it on `main`.
2. `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings`
   green locally; CI runs the same and the tag build uses `--locked`, so
   `Cargo.lock` must be committed with the bump.
3. Push `main` first, then the tag:

   ```
   git tag vX.Y.Z
   git push origin main
   git push origin vX.Y.Z
   ```

4. Watch the `build` workflow. A failed tag build leaves no release; fix on
   `main`, move the tag, push it again, and the release job rebuilds in
   place.
5. If the release adds a field to the version grammar, install it on every
   machine sharing a store before the first write that uses the field: an
   older binary reads such a file as corrupt.

Nothing checks that the tag matches `Cargo.toml`; `worklog --version`
reports the manifest, so a mismatch is visible but not fatal.
