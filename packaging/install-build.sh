#!/bin/sh
# Installs a build of this checkout where get.sh installs a release, named
# after its commit so that it is told apart from the release it was made on.
set -eu

prefix="${PREFIX:-$HOME/.local/bin}"

cd "$(dirname "$0")/.."

build="$(git rev-parse --short HEAD)"
git diff --quiet HEAD || build="$build.dirty"

WORKLOG_BUILD="$build" cargo build --release --locked

mkdir -p "$prefix"
install -m 0755 target/release/worklog "$prefix/worklog"
"$prefix/worklog" refresh
