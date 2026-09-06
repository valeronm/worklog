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

# The agent skill describes the commands of the binary it shipped with, so a
# an agent that has one gets the new one; an agent that never opted in is
# left alone.
for skills in "$HOME/.claude/skills" "$HOME/.codex/skills"; do
  if [ -f "$skills/worklog/SKILL.md" ]; then
    "$prefix/worklog" skill install --dir "$skills" >/dev/null
    echo "agent skill updated in $skills/worklog"
  fi
done

# Completions are generated from the binary, so they are written where a
# shell already looks for them and rewritten with every install.
fish_completions="${XDG_CONFIG_HOME:-$HOME/.config}/fish/completions"
if [ -d "$fish_completions" ]; then
  "$prefix/worklog" completions fish > "$fish_completions/worklog.fish"
  echo "fish completions written to $fish_completions/worklog.fish"
fi
