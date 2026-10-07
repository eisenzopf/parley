#!/usr/bin/env bash
# Publish a verified stage HTML file without rebuilding/restarting Parley.
set -euo pipefail
bucket="${1:?artifact bucket required}"
key="${2:?HTML artifact key required}"
digest="${3:?HTML SHA-256 required}"
[[ "$digest" =~ ^[a-f0-9]{64}$ ]] || exit 2
stage="$(mktemp -d /opt/parley/stage-html.XXXXXX)"
trap 'rm -rf "$stage"' EXIT
target=/opt/parley/current/web/conference/index.html
aws s3 cp "s3://$bucket/$key" "$stage/index.html" --only-show-errors
printf '%s  %s\n' "$digest" "$stage/index.html" | sha256sum --check
grep -q 'id="kit-share"' "$stage/index.html"
grep -q 'https://conference.rudeless.ai/kit/' "$stage/index.html"
grep -q 'conference.mjs' "$stage/index.html"
install -d -o root -g root -m 700 /opt/parley/static-backups
previous="$(sha256sum "$target" | cut -d ' ' -f 1)"
if [ ! -f "/opt/parley/static-backups/$previous.html" ]; then
  install -o root -g root -m 600 "$target" "/opt/parley/static-backups/$previous.html"
fi
install -o parley -g parley -m 644 "$stage/index.html" "$target.next"
mv -f "$target.next" "$target"
if curl --fail --silent https://conference.rudeless.ai/conference/ -o "$stage/served.html" && printf '%s  %s\n' "$digest" "$stage/served.html" | sha256sum --check; then
  echo "Published stage HTML $digest; previous HTML $previous retained. Parley binary/service unchanged."
else
  install -o parley -g parley -m 644 "/opt/parley/static-backups/$previous.html" "$target.next"
  mv -f "$target.next" "$target"
  echo 'Stage HTML verification failed; previous HTML restored.' >&2
  exit 1
fi
