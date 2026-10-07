"""Build a source-only attendee snapshot. Never archive the workspace wholesale."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile

ROOT_FILES = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "package.json",
              "package-lock.json", "playwright.config.ts", "README.md", "LICENSE",
              ".gitignore", ".env.example", "CONFERENCE_DEMO_PLAN.md", "PRD.md", "IMPLEMENTATION_PLAN.md"]
SOURCE_DIRS = ["src", "tests", "migrations", "config", "clients", "examples", "scripts",
               "web", "vendor", "patches", "docs", "e2e", "infra", ".github"]
SUFFIXES = {".rs", ".mjs", ".js", ".ts", ".html", ".css", ".json", ".toml", ".lock",
            ".md", ".yml", ".yaml", ".py", ".sh", ".patch", ".sql", ".swift"}
SKIP = {"node_modules", "__pycache__", ".DS_Store"}


def source_files(root):
    paths = [root / name for name in ROOT_FILES]
    for name in SOURCE_DIRS:
        directory = root / name
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"Required source directory missing: {name}")
        for current, dirs, files in os.walk(directory, followlinks=False):
            dirs[:] = sorted(d for d in dirs if d not in SKIP)
            for child in dirs:
                if (Path(current) / child).is_symlink():
                    raise ValueError(f"Refusing source symlink: {Path(current) / child}")
            for child in sorted(files):
                path = Path(current) / child
                if child in SKIP:
                    continue
                if path.is_symlink():
                    raise ValueError(f"Refusing source symlink: {path}")
                if path.suffix not in SUFFIXES and child not in {"LICENSE", "BASE_REV", ".gitignore"}:
                    raise ValueError(f"Unexpected source file; review before sharing: {path.relative_to(root)}")
                paths.append(path)
    for path in paths:
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Required regular source file missing: {path}")
    return sorted(paths)


def build(root, destination, secrets=()):
    if destination.exists():
        raise ValueError(f"Refusing to overwrite existing output: {destination}")
    contents = {}
    for path in source_files(root):
        data = path.read_bytes()
        if any(value and len(value) >= 8 and value.encode() in data for value in secrets):
            raise ValueError(f"Known credential found in source: {path.relative_to(root)}")
        contents[str(path.relative_to(root))] = data
    files = {name: hashlib.sha256(data).hexdigest() for name, data in contents.items()}
    source_digest = hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()
    base = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    manifest = {"version": 1, "kind": "experimental-conference-working-tree-snapshot",
                "base_commit": base, "source_sha256": source_digest,
                "profile": "conversation-control/1", "transport": "WSS (QUIC not demonstrated)",
                "rvoip": json.loads(contents["config/conference-dependencies.json"])["rvoip"],
                "files": files, "excluded": ["credentials", "contacts except fictional fixtures", "runtime state",
                                             "recordings", "provider evidence", "Git metadata", "build outputs"]}
    contents["KIT_MANIFEST.json"] = json.dumps(manifest, indent=2).encode()
    destination.mkdir(parents=True, exist_ok=False)
    archive = destination / "parley-conference-kit.tar.gz"
    with archive.open("wb") as output, gzip.GzipFile(fileobj=output, mode="wb", filename="", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as tar:
            for name, data in sorted(contents.items()):
                entry = tarfile.TarInfo(f"parley/{name}")
                entry.size = len(data)
                entry.mode = 0o755 if name.endswith(".sh") else 0o644
                entry.mtime = 0
                tar.addfile(entry, io.BytesIO(data))
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (destination / "SHA256SUMS").write_text(f"{digest}  {archive.name}\n")
    (destination / "manifest.json").write_text(json.dumps({**manifest, "archive_sha256": digest}, indent=2))
    for name in ["CONTRIBUTING_CONNECTORS.md", "UCTP_CONFERENCE_PROFILE.md", "CONFERENCE_RUNBOOK.md", "CONFERENCE_VOICE_REHEARSAL.md", "CONFERENCE_IMPLEMENTATION_STATUS.md"]:
        (destination / name).write_bytes(contents[f"docs/{name}"])
    (destination / "client.mjs").write_bytes(contents["clients/uctp-js/client.mjs"])
    (destination / "index.html").write_bytes(contents["web/conference-kit/index.html"])
    return {"files": len(files), "source_sha256": source_digest, "archive_sha256": digest,
            "directory": str(destination)}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="New output directory; existing outputs are never overwritten")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    secrets = [value for name, value in os.environ.items() if any(part in name for part in ["KEY", "TOKEN", "SECRET", "PASSWORD"])]
    print(json.dumps(build(root, args.destination.resolve(), secrets), indent=2))
