# Cutting a release

A release is a tag. Pushing `vX.Y.Z` runs the checks, builds the root
package's binary for Linux x86_64 and aarch64 and macOS arm64 on their own
runners, and creates or updates the GitHub release with each binary and
its `.sha256`. `packaging/get.sh` and `worklog upgrade` both resolve the
latest release from the redirect and install from it.

1. Bump `version` in the root `Cargo.toml` and commit it on `main` with
   `Cargo.lock`, since the tag build uses `--locked`.
2. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
   --all-features -- -D warnings` and `cargo test --workspace` green
   locally; CI runs the same.
3. Push `main` first, then the tag:

   ```
   git tag vX.Y.Z
   git push origin main
   git push origin vX.Y.Z
   ```

4. Watch the `build` workflow. A failed tag build leaves no release; fix
   on `main`, move the tag, push it again, and the release job rebuilds in
   place.

The tag must be a plain `vX.Y.Z`: `upgrade` reads it as three numbers to
decide whether the release is newer than the binary, and refuses any other
shape. Nothing checks that the tag matches `Cargo.toml`. A binary reports
the manifest's version, so a tag ahead of the manifest has `upgrade`
install the same release again on every run.

A binary compiled with `WORKLOG_BUILD` set to a name of letters, digits
and dots is a build made on top of a release. It reports `X.Y.Z+<name>`,
and `upgrade` counts it as newer than the release `X.Y.Z`, so only a
release of a higher number takes its place. The workflow compiles a
release with the variable empty, which reads as unset.
`packaging/install-build.sh` installs such a build, named after the commit
it was made from.

## A change to what is stored

Machines sharing a store run whatever binary each has, so a release that
changes what a version file holds meets older readers. `store.md` has
what a reader tolerates; what follows from it:

- A new reason for an ending and a new `change` word are read by an older
  binary as the words they are. Documents of a new kind are kept by it,
  counted by its `check` in a notice, and not interpreted.
- A new key in the header, or a new field on a kind, is not read by an
  older binary: its `check` reports the version as a problem, and it
  refuses to change the document. Raising `FORMAT` in `domain::version`
  makes the report a notice and still refuses the change. Either way,
  upgrade every machine sharing the store before the first document is
  written in the new shape.
- A new key in a usage line is passed over by an older reader.
- The host's cache is rebuilt by the first walk of a binary that did not
  write it, so a release owes it nothing.

## A store from before 0.5.0

0.5.0 keeps another store format and another host state than 0.4, and
reads neither of them: `init` refuses a directory that holds a 0.4 store.
The converter and the steps of the crossing are in the tree at the tag
`v0.5.0`, as `crates/worklog-convert` and this file. A store converted
there is read by every later release.
