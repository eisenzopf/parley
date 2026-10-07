#!/usr/bin/env bash
# Run through SSM on the conference host. Only publishes source-only static assets.
set -euo pipefail
bucket="${1:?artifact bucket required}"
key="${2:?artifact key required}"
digest="${3:?artifact SHA-256 required}"
[[ "$digest" =~ ^[a-f0-9]{64}$ ]] || exit 2
stage="$(mktemp -d /opt/parley/kit-stage.XXXXXX)"
trap 'rm -rf "$stage"' EXIT
aws s3 cp "s3://$bucket/$key" "$stage/public.tar.gz" --only-show-errors
printf '%s  %s\n' "$digest" "$stage/public.tar.gz" | sha256sum --check
python3 - "$stage" <<'PY'
import hashlib,json,sys,tarfile
from pathlib import Path
stage=Path(sys.argv[1]);target=stage/'files';target.mkdir()
allowed={'index.html','parley-conference-kit.tar.gz','manifest.json','SHA256SUMS','client.mjs',
         'CONTRIBUTING_CONNECTORS.md','UCTP_CONFERENCE_PROFILE.md','CONFERENCE_RUNBOOK.md',
         'CONFERENCE_VOICE_REHEARSAL.md','CONFERENCE_IMPLEMENTATION_STATUS.md','join-qr.png','join-qr.svg'}
with tarfile.open(stage/'public.tar.gz') as archive:
    entries=archive.getmembers()
    names=[entry.name for entry in entries]
    if len(set(names))!=len(names) or not set(names)<=allowed or not allowed-{'join-qr.png','join-qr.svg'}<=set(names):
        raise SystemExit('Unexpected or missing public assets')
    for entry in entries:
        if not entry.isfile():raise SystemExit('Only regular files allowed')
        (target/entry.name).write_bytes(archive.extractfile(entry).read())
manifest=json.loads((target/'manifest.json').read_text())
if hashlib.sha256((target/'parley-conference-kit.tar.gz').read_bytes()).hexdigest()!=manifest['archive_sha256']:
    raise SystemExit('Source archive manifest mismatch')
PY
release="/srv/parley-public/kits/$digest"
install -d -o root -g caddy -m 755 /srv/parley-public /srv/parley-public/kits
if [ ! -d "$release" ]; then mv "$stage/files" "$release"; fi
chown -R root:caddy "$release"
find "$release" -type d -exec chmod 755 {} +
find "$release" -type f -exec chmod 644 {} +
python3 - /etc/caddy/Caddyfile "$stage/Caddyfile" <<'PY'
from pathlib import Path
import sys
original=Path(sys.argv[1]).read_text()
if 'conference.rudeless.ai {' not in original:raise SystemExit('Unexpected conference edge configuration')
block='''
  handle /kit {
    redir * /kit/ 308
  }
  handle_path /kit/* {
    root * /srv/parley-public/kit
    header X-Content-Type-Options nosniff
    header Referrer-Policy no-referrer
    header Content-Security-Policy "default-src 'none'; style-src 'unsafe-inline'; img-src 'self'; base-uri 'none'; frame-ancestors 'none'"
    file_server
  }
'''
if 'handle_path /kit/*' not in original:
    original=original.replace('conference.rudeless.ai {','conference.rudeless.ai {'+block,1)
elif 'root * /srv/parley-public/kit' not in original:
    raise SystemExit('Existing kit route uses an unexpected directory')
Path(sys.argv[2]).write_text(original)
PY
caddy validate --config "$stage/Caddyfile" --adapter caddyfile
previous="$(readlink /srv/parley-public/kit || true)"
if [ -e /srv/parley-public/kit ] && [ ! -L /srv/parley-public/kit ]; then echo 'Unexpected existing kit directory' >&2; exit 1; fi
cp /etc/caddy/Caddyfile "$stage/previous-Caddyfile"
config_changed=0
if ! cmp -s /etc/caddy/Caddyfile "$stage/Caddyfile"; then config_changed=1; fi
ln -s "$release" "$stage/next-kit"
mv -Tf "$stage/next-kit" /srv/parley-public/kit
activate_config() {
  if [ "$config_changed" -eq 1 ]; then
    install -o root -g caddy -m 640 "$stage/Caddyfile" /etc/caddy/Caddyfile && systemctl reload caddy
  fi
}
# Updating files under an existing kit route needs no reload, which also keeps
# the presenter and worker's active WebSocket control connections intact.
if activate_config && curl --fail --silent https://conference.rudeless.ai/kit/ >/dev/null && curl --fail --silent https://conference.rudeless.ai/healthz >/dev/null; then
  echo "Published static kit $digest; Parley runtime unchanged."
else
  if [ "$config_changed" -eq 1 ]; then
    install -o root -g caddy -m 640 "$stage/previous-Caddyfile" /etc/caddy/Caddyfile
  fi
  if [ -n "$previous" ]; then ln -s "$previous" "$stage/previous-kit"; mv -Tf "$stage/previous-kit" /srv/parley-public/kit; else rm /srv/parley-public/kit; fi
  if [ "$config_changed" -eq 1 ]; then systemctl reload caddy; fi
  echo 'Kit publication failed; previous edge configuration restored.' >&2
  exit 1
fi
