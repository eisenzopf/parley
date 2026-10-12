#!/usr/bin/env bash
# Install the authenticated browser controller without restarting Parley/media.
set -euo pipefail
umask 077
bucket=${1:?bucket} code_key=${2:?code key} code_sha=${3:?code SHA} node_key=${4:?Node key} node_sha=${5:?Node SHA} release=${6:?release}
[[ "$code_sha" =~ ^[a-f0-9]{64}$ && "$node_sha" =~ ^[a-f0-9]{64}$ && "$release" =~ ^[a-zA-Z0-9-]+$ ]]
root=/opt/parley/browser-manager/releases/$release
archive=/opt/parley/build/$release-browser.tar.gz
node_archive=/opt/parley/build/$release-node.tar.xz
test ! -e "$root"
aws s3 cp "s3://$bucket/$code_key" "$archive" --only-show-errors
aws s3 cp "s3://$bucket/$node_key" "$node_archive" --only-show-errors
printf '%s  %s\n' "$code_sha" "$archive" | sha256sum --check
printf '%s  %s\n' "$node_sha" "$node_archive" | sha256sum --check
install -d -o root -g parley -m 750 "$root" /opt/parley/node
python3 - "$archive" "$root" <<'PY'
import pathlib,sys,tarfile
with tarfile.open(sys.argv[1]) as tar:
 for m in tar.getmembers():
  p=pathlib.PurePosixPath(m.name)
  if not m.isfile() or p.is_absolute() or '..' in p.parts or p.parts[0]!='parley' or p.suffix!='.mjs':raise SystemExit('Invalid manager source archive')
  target=pathlib.Path(sys.argv[2])/p;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(tar.extractfile(m).read());target.chmod(0o644)
PY
tar -xJf "$node_archive" -C /opt/parley/node --strip-components=1 --no-same-owner
/opt/parley/node/bin/node --version
chown -R root:parley /opt/parley/browser-manager /opt/parley/node
chmod -R g+rX /opt/parley/browser-manager /opt/parley/node
install -d -o parley -g parley -m 700 /opt/parley/state/browser-manager
ln -sfn "$root/parley" /opt/parley/browser-manager/current
cat > /etc/parley/browser-manager.env <<'ENV'
CONFERENCE_BROWSER_STATE_DIR=/opt/parley/state/browser-manager
CONFERENCE_BROWSER_ROSTER=/opt/parley/state/browser-roster.json
CONFERENCE_BROWSER_ORIGIN=https://conference.rudeless.ai
CONFERENCE_BROWSER_SENDER_CONFIG=/opt/parley/state/browser-sender-config.json
CONFERENCE_BROWSER_IMPORT=/opt/parley/state/browser-import.json
CONFERENCE_BROWSER_IMPORT_WORKER=/opt/parley/state/browser-import-worker.json
CONFERENCE_BROWSER_PORT=8090
PARLEY_HTTP_URL=http://127.0.0.1:8080
UCTP_URL=wss://conference.rudeless.ai/uctp
CONFERENCE_DEMO_MODE=full
ENV
chmod 600 /etc/parley/browser-manager.env
cat > /etc/systemd/system/parley-browser.service <<'UNIT'
[Unit]
Description=Parley browser access and scoped assistant supervision
After=network-online.target parley.service
Wants=network-online.target
[Service]
User=parley
Group=parley
WorkingDirectory=/opt/parley/browser-manager/current
EnvironmentFile=/etc/parley/runtime.env
EnvironmentFile=/etc/parley/browser-manager.env
ExecStart=/opt/parley/node/bin/node examples/conference-assistant/browser-manager.mjs
Restart=on-failure
RestartSec=5
KillMode=control-group
TimeoutStopSec=10
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/parley/state
[Install]
WantedBy=multi-user.target
UNIT
cp /etc/caddy/Caddyfile "$root/Caddyfile.before"
python3 - <<'PY'
from pathlib import Path
p=Path('/etc/caddy/Caddyfile');s=p.read_text()
if '@browser_manager' in s:
 if 'reverse_proxy 127.0.0.1:8090' not in s:raise SystemExit('Unexpected manager route')
else:
 needle='  @conference path '
 if needle not in s:raise SystemExit('Expected conference route missing')
 s=s.replace(needle,'  @browser_manager path /v1/conference/browser/*\n  handle @browser_manager {\n    reverse_proxy 127.0.0.1:8090\n  }\n'+needle,1)
 p.write_text(s)
PY
/usr/local/bin/caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
systemctl daemon-reload
systemctl enable parley-browser
systemctl restart parley-browser
systemctl reload caddy
systemctl is-active parley-browser parley caddy
curl --silent --show-error --retry 10 --retry-connrefused --retry-delay 1 http://127.0.0.1:8090/v1/conference/browser/session
printf '\nBrowser manager installed; access still requires a private one-use browser link.\n'
