#!/bin/sh
# Installs the latest worklog release into ~/.local/bin, verifying the
# checksum published beside the binary. POSIX sh, so it can be piped from
# curl on a machine with nothing else set up yet.
set -eu

repo="valeronm/worklog"
prefix="${PREFIX:-$HOME/.local/bin}"

command -v curl >/dev/null 2>&1 || { echo "get.sh: curl is required" >&2; exit 1; }

# Linux ships coreutils' sha256sum and often no shasum, macOS the reverse.
if command -v sha256sum >/dev/null 2>&1; then
  verify_checksum() { sha256sum -c "$1"; }
elif command -v shasum >/dev/null 2>&1; then
  verify_checksum() { shasum -a 256 -c "$1"; }
else
  echo "get.sh: sha256sum or shasum is required" >&2
  exit 1
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) asset="worklog-x86_64-linux" ;;
  Linux-aarch64) asset="worklog-aarch64-linux" ;;
  Darwin-arm64) asset="worklog-aarch64-darwin" ;;
  *)
    echo "get.sh: no release for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

# The latest release is a redirect, so the tag is read from where it lands
# rather than from the API, which is rate-limited when unauthenticated.
tag=$(curl -fsSIL -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest" | sed 's#.*/##')
if [ -z "$tag" ]; then
  echo "get.sh: could not resolve the latest release" >&2
  exit 1
fi
base="https://github.com/$repo/releases/download/$tag"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/$asset" "$base/$asset"
curl -fsSL -o "$tmp/$asset.sha256" "$base/$asset.sha256"
(cd "$tmp" && verify_checksum "$asset.sha256" >/dev/null)

mkdir -p "$prefix"
install -m 0755 "$tmp/$asset" "$prefix/worklog"
echo "worklog $tag installed to $prefix/worklog"

# Captured rather than piped so its exit status is the script's.
finished="$("$prefix/worklog" agents refresh)"
if [ -n "$finished" ]; then
  printf '%s\n' "$finished" | sed 's/^/updated /'
fi
