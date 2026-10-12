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

## 0.5.0, the rebuilt store

0.5.0 keeps another store format and another host state than 0.4, and
reads neither of them. A store and its hosts cross over in this order.

1. On every machine, save or discard the drafts open under the 0.4
   binary, and let the file sync settle. 0.5.0 does not read those
   drafts, and what a 0.4 binary writes into the old store after the
   conversion is not carried over.
2. On one machine, with a 0.4 binary still installed, build
   `crates/worklog-convert` from source and run its dry run until it
   reports nothing open; its README has the run. What the dry run sends
   back to the old store is mended with the 0.4 binary, which an upgraded
   machine no longer has. Then convert into a new directory: the
   converter refuses the old store's own, since a 0.4 binary pointed at a
   converted store reads it as empty and writes its own files into it. For
   the same reason, do not move the new store to the old one's path while
   a 0.4 binary is still configured for it. Run `worklog check` on the
   result with the 0.5 binary built from the same source. `check` answers
   only on a bound host, so bind a temporary one: set `WORKLOG_HOME` to an
   empty directory and run `init` as in step 5 under it.
3. Share the new directory between the machines by the file sync.
4. Upgrade each machine: `worklog upgrade` from 0.3.0 on, the installer
   otherwise. Either runs the new binary's `agents refresh`, which takes
   over what the 0.4 binary placed at the same paths: the skill, the
   SessionStart hook and the fish completions. The upgrade prints nothing
   about setup: every command that uses the store answers that the host
   is not set up until `init`, and `context` prints that as a note and
   exits 0, so sessions still open.
5. On each machine, once the sync has delivered the converted store, bind
   the host with `worklog init <machine topic> --store <dir>` and no
   `--summary`. Before the store has arrived `init` accepts the missing or
   empty directory, and `--summary` then makes a new topic of that name,
   a second one once the sync delivers the converted store; without it a
   name the store does not hold is refused. `init` also refuses the old
   store's directory, since it holds files and no store of this format,
   and leaves the earlier config, another file, as it is.
