"""Generate a conference QR only after verifying its public kit destination."""
import argparse
import hashlib
import json
from pathlib import Path
from urllib.parse import urljoin, urlsplit
from urllib.request import urlopen
import qrcode
import qrcode.image.svg

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('url')
parser.add_argument('output', type=Path)
args = parser.parse_args()
url = urlsplit(args.url)
if url.scheme != 'https' or url.username or url.password or url.query or url.fragment or not args.url.endswith('/'):
    raise SystemExit('Public HTTPS kit URL ending in / required')
with urlopen(args.url, timeout=20) as response:
    if response.status != 200 or b'Bring the next' not in response.read():
        raise SystemExit('Public kit landing page not verified')
with urlopen(urljoin(args.url, 'manifest.json'), timeout=20) as response:
    manifest = json.load(response)
with urlopen(urljoin(args.url, 'parley-conference-kit.tar.gz'), timeout=20) as response:
    if hashlib.sha256(response.read()).hexdigest() != manifest['archive_sha256']:
        raise SystemExit('Public kit archive checksum mismatch')
args.output.mkdir(parents=True, exist_ok=False)
qr = qrcode.QRCode(error_correction=qrcode.constants.ERROR_CORRECT_H, box_size=12, border=4)
qr.add_data(args.url)
qr.make(fit=True)
qr.make_image(fill_color='black', back_color='white').save(args.output / 'join-qr.png')
qr.make_image(image_factory=qrcode.image.svg.SvgPathFillImage).save(args.output / 'join-qr.svg')
(args.output / 'qr-evidence.json').write_text(json.dumps({'url': args.url, 'archive_sha256': manifest['archive_sha256'],
    'destination_verified': True, 'generator': 'qrcode==8.2', 'decoder_verified': False}, indent=2))
print('Public destination and download verified; QR generated. Independently decode before displaying.')
