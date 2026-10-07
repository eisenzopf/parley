#!/usr/bin/env bash
# Reproduce the local 0.3.12 + conference patch checkout without changing ../rvoip.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dest="${PARLEY_RVOIP_DIR:-$(dirname "$root")/rvoip-conference}"
base="$(cat "$root/patches/rvoip/BASE_REV")"
if [ ! -d "$dest/.git" ]; then
  if [ -e "$dest" ]; then
    echo "Refusing to overwrite $dest" >&2
    exit 1
  fi
  git clone --no-checkout https://github.com/eisenzopf/rvoip.git "$dest"
  git -C "$dest" checkout --detach "$base"
fi
if [ "$(git -C "$dest" rev-parse HEAD)" != "$base" ]; then
  echo "Expected rvoip baseline $base at $dest; use a separate checkout." >&2
  exit 1
fi
patch="$root/patches/rvoip/conference.patch"
if [ -s "$patch" ]; then
  if git -C "$dest" apply --reverse --check "$patch" 2>/dev/null; then
    echo "Conference patch already applied."
  else
    git -C "$dest" apply --check "$patch"
    git -C "$dest" apply "$patch"
  fi
fi
echo "rvoip 0.3.12 + local conference patches: $dest"
