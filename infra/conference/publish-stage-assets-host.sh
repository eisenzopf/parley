#!/usr/bin/env bash
# Replace only public stage assets; keep the running binary and calls unchanged.
set -euo pipefail
bucket="${1:?artifact bucket required}" key="${2:?artifact key required}" digest="${3:?SHA-256 required}"
[[ "$digest" =~ ^[a-f0-9]{64}$ ]] || exit 2
stage="$(mktemp -d /opt/parley/stage-assets.XXXXXX)"
trap 'rm -rf "$stage"' EXIT
aws s3 cp "s3://$bucket/$key" "$stage/assets.tar.gz" --only-show-errors
printf '%s  %s\n' "$digest" "$stage/assets.tar.gz" | sha256sum --check
python3 - "$stage/assets.tar.gz" "$digest" <<'PY'
import hashlib,os,pathlib,pwd,shutil,sys,tarfile,tempfile,urllib.request
names={'index.html','conference.mjs','projection.mjs','network.mjs','ringtone.mjs'}
root=pathlib.Path('/opt/parley/current/web/conference')
backup=pathlib.Path('/opt/parley/static-backups')/('assets-'+sys.argv[2])
if backup.exists(): raise SystemExit('Immutable asset backup already exists; inspect prior deployment before retrying')
with tarfile.open(sys.argv[1]) as tar:
    members=tar.getmembers()
    if len(members)!=len(names) or {m.name for m in members}!=names or any(not m.isfile() for m in members):
        raise SystemExit('Unexpected stage asset archive')
    contents={m.name:tar.extractfile(m).read() for m in members}
if b'id="kit-share"' not in contents['index.html'] or b'conference.mjs' not in contents['index.html']:
    raise SystemExit('Unexpected stage HTML')
backup.mkdir(parents=True,mode=0o700)
for name in names:
    if (root/name).exists():
        shutil.copyfile(root/name,backup/name); (backup/name).chmod(0o600)
user=pwd.getpwnam('parley')
def replace(name,data):
    fd,path=tempfile.mkstemp(prefix='stage-next-',dir=root)
    try:
        with os.fdopen(fd,'wb') as f: f.write(data)
        os.chown(path,user.pw_uid,user.pw_gid); os.chmod(path,0o644); os.replace(path,root/name)
    finally:
        if os.path.exists(path): os.unlink(path)
try:
    for name in sorted(names): replace(name,contents[name])
    for name in sorted(names):
        with urllib.request.urlopen('https://conference.rudeless.ai/conference/'+name,timeout=10) as r: actual=r.read()
        if actual!=contents[name]: raise RuntimeError('Served asset mismatch: '+name)
    with urllib.request.urlopen('https://conference.rudeless.ai/healthz',timeout=10) as r:
        if r.status!=200: raise RuntimeError('Health check failed')
except BaseException:
    for name in names:
        if (backup/name).exists(): replace(name,(backup/name).read_bytes())
        elif (root/name).exists(): (root/name).unlink()
    raise
print('Published stage assets; Parley service and binary unchanged.')
for name in sorted(names): print(name,hashlib.sha256(contents[name]).hexdigest())
PY
