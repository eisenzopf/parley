#!/bin/bash
# Update the dedicated idle conference host; preserve runtime configuration/Caddy.
# Run as root through SSM. Source archive has parley/ and rvoip-conference/ roots.
set -euo pipefail
umask 027
bucket=$1 artifact=$2 expected_sha=$3 release=$4
[[ "$expected_sha" =~ ^[a-f0-9]{64}$ ]]
[[ "$release" =~ ^[a-zA-Z0-9-]+$ ]]
root="/opt/parley/releases/$release"
test ! -e "$root"
previous=$(readlink -f /opt/parley/current)
test -x "$previous/parley"
archive="/opt/parley/build/$release.tar.gz"
aws s3 cp "s3://$bucket/$artifact" "$archive" --region us-west-2 --only-show-errors
printf '%s  %s\n' "$expected_sha" "$archive" | sha256sum --check
install -d -o parley -g parley -m 750 "$root"
python3 - "$archive" "$root" <<'PY'
import pathlib,sys,tarfile,shutil
with tarfile.open(sys.argv[1]) as tar:
    members=tar.getmembers()
    for member in members:
        path=pathlib.PurePosixPath(member.name)
        if not member.isfile() or path.is_absolute() or '..' in path.parts or path.parts[0] not in ['parley','rvoip-conference']:
            raise SystemExit('Unsafe source archive member')
    for member in members:
        target=pathlib.Path(sys.argv[2])/member.name
        target.parent.mkdir(parents=True,exist_ok=True)
        with tar.extractfile(member) as source,target.open('wb') as output:
            shutil.copyfileobj(source,output)
        target.chmod(0o755 if member.name.endswith('.sh') else 0o644)
PY
chown -R parley:parley "$root"
su -s /bin/bash parley -c "cd '$root/parley' && PATH=/opt/parley/.cargo/bin:\$PATH CARGO_TARGET_DIR=/opt/parley/build/target CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_INCREMENTAL=0 /opt/parley/.cargo/bin/cargo build --locked --release -j 4" > "/opt/parley/build/$release.log" 2>&1
install -o parley -g parley -m 750 /opt/parley/build/target/release/parley "$root/parley/parley"
sha256sum "$root/parley/parley" > "$root/binary.sha256"
# Check actual persistent state immediately before any service interruption.
python3 - <<'PY'
import sqlite3
db=sqlite3.connect('file:/opt/parley/state/parley.sqlite?mode=ro',uri=True)
voice=db.execute("SELECT count(*) FROM sessions WHERE state NOT IN ('ended','failed','interrupted')").fetchone()[0]
sms=db.execute("SELECT count(*) FROM message_deliveries WHERE state IN ('queued','submitting','unknown')").fetchone()[0]
if voice or sms: raise SystemExit('Host has active voice or unsettled SMS; update not activated')
print('Idle-state gate passed')
PY
switched=0
rollback() {
  rc=$?
  if [ "$rc" -ne 0 ] && [ "$switched" -eq 1 ]; then
    ln -sfn "$previous" /opt/parley/current
    systemctl restart parley
    echo 'Update failed; restored previous release' >&2
  fi
  exit "$rc"
}
trap rollback EXIT
switched=1
systemctl stop parley
# A consistent private recovery snapshot; state remains at its original location.
python3 - "$root/state-before-update.sqlite" <<'PY'
import sqlite3,sys,os
os.umask(0o077)
with sqlite3.connect('/opt/parley/state/parley.sqlite') as old, sqlite3.connect(sys.argv[1]) as backup:
    old.backup(backup)
PY
ln -sfn "$root/parley" /opt/parley/current
systemctl start parley
curl --fail --silent --show-error --retry 15 --retry-connrefused --retry-delay 1 --max-time 5 http://127.0.0.1:8080/healthz
systemctl is-active parley caddy
sha256sum "$root/parley/parley"
printf 'Active release: %s\nPrevious release: %s\n' "$root/parley" "$previous"
