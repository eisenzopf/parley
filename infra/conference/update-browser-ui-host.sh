#!/usr/bin/env bash
# Update browser management and public UI without restarting the media server.
set -euo pipefail
umask 027
bucket=${1:?bucket} key=${2:?key} digest=${3:?digest} release=${4:?release}
[[ "$digest" =~ ^[a-f0-9]{64}$ && "$release" =~ ^[a-zA-Z0-9-]+$ ]]
root=/opt/parley/browser-manager/releases/$release
archive=/opt/parley/build/$release.tar.gz
previous=$(readlink -f /opt/parley/browser-manager/current)
test ! -e "$root"
aws s3 cp "s3://$bucket/$key" "$archive" --only-show-errors
printf '%s  %s\n' "$digest" "$archive" | sha256sum --check
python3 - "$archive" "$root" <<'PY'
from pathlib import Path,PurePosixPath
import sys,tarfile
with tarfile.open(sys.argv[1]) as tar:
 for m in tar.getmembers():
  p=PurePosixPath(m.name)
  if not m.isfile() or p.is_absolute() or '..' in p.parts or p.parts[0]!='parley' or p.suffix not in ['.mjs','.html']: raise SystemExit('Invalid UI archive')
  target=Path(sys.argv[2])/p;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(tar.extractfile(m).read());target.chmod(0o644)
PY
chown -R root:parley "$root"
chmod -R g+rX "$root"
/opt/parley/node/bin/node --check "$root/parley/examples/conference-assistant/browser-manager.mjs"
python3 - <<'PY'
import sqlite3
c=sqlite3.connect('file:/opt/parley/state/parley.sqlite?mode=ro',uri=True)
if c.execute("SELECT count(*) FROM sessions WHERE state NOT IN ('ended','failed','interrupted')").fetchone()[0] or c.execute("SELECT count(*) FROM message_deliveries WHERE state IN ('queued','submitting','unknown')").fetchone()[0]: raise SystemExit('Active work; update not activated')
PY
# Back up every public file before switching either component.
python3 - "$root" <<'PY'
from pathlib import Path
import shutil,sys
root=Path(sys.argv[1]);current=Path('/opt/parley/current')
for name in ['web/conference/index.html','web/conference/conference.mjs','clients/uctp-js/browser-audio.mjs']:
 target=root/'public-before'/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(current/name,target)
PY
switched=0
rollback() {
 rc=$?
 if [ "$rc" -ne 0 ] && [ "$switched" -eq 1 ]; then
  cp "$root/public-before/web/conference/"* /opt/parley/current/web/conference/
  cp "$root/public-before/clients/uctp-js/browser-audio.mjs" /opt/parley/current/clients/uctp-js/browser-audio.mjs
  ln -sfn "$previous" /opt/parley/browser-manager/current
  systemctl restart parley-browser
 fi
 exit "$rc"
}
trap rollback EXIT
switched=1
systemctl stop parley-browser
ln -sfn "$root/parley" /opt/parley/browser-manager/current
for name in web/conference/index.html web/conference/conference.mjs clients/uctp-js/browser-audio.mjs; do
 install -o parley -g parley -m 644 "$root/parley/$name" "/opt/parley/current/$name.next"
 mv "/opt/parley/current/$name.next" "/opt/parley/current/$name"
done
systemctl start parley-browser
curl --silent --show-error --retry 10 --retry-connrefused --retry-delay 1 http://127.0.0.1:8090/v1/conference/browser/access > "$root/access.json"
python3 - "$root" <<'PY'
from pathlib import Path
import json,sys,urllib.request,urllib.error
root=Path(sys.argv[1])
assert json.loads((root/'access.json').read_text()).get('error')=='Sign in with your Parley password.'
for path,name in [('conference/index.html','web/conference/index.html'),('conference/conference.mjs','web/conference/conference.mjs'),('uctp-client/browser-audio.mjs','clients/uctp-js/browser-audio.mjs')]:
 with urllib.request.urlopen('https://conference.rudeless.ai/'+path,timeout=10) as r: assert r.read()==(root/'parley'/name).read_bytes(),path
try: urllib.request.urlopen('http://127.0.0.1:8090/v1/conference/browser/phones',timeout=5);raise RuntimeError('Phone settings missing authentication')
except urllib.error.HTTPError as e: assert e.code==401
PY
systemctl is-active parley-browser parley caddy
printf 'Browser UI release: %s\nPrevious: %s\n' "$root" "$previous"
