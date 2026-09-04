#!/bin/sh
# Installs the latest worklog release into ~/.local/bin, verifying the
# checksum published beside the binary. POSIX sh, so it can be piped from
# curl on a machine with nothing else set up yet.
set -eu

repo="valeronm/worklog"
prefix="${PREFIX:-$HOME/.local/bin}"

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
(cd "$tmp" && shasum -a 256 -c "$asset.sha256" >/dev/null)

mkdir -p "$prefix"
install -m 0755 "$tmp/$asset" "$prefix/worklog"
echo "worklog $tag installed to $prefix/worklog"
