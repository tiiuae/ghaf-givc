#!/usr/bin/env python3

import json
import subprocess
import sys
from pathlib import Path


MANIFEST_MEDIA_TYPE = "application/vnd.ghaf.ota.manifest.v1+json"
UKI_MEDIA_TYPE = "application/vnd.ghaf.ota.uki.v1+efi"
ROOT_MEDIA_TYPE = "application/vnd.ghaf.ota.root.v1+raw"
VERITY_MEDIA_TYPE = "application/vnd.ghaf.ota.verity.v1+raw"
CHANGELOG_MEDIA_TYPE = "application/vnd.ghaf.ota.changelog.v1+plain"


def usage() -> int:
    print(
        f"usage: {Path(sys.argv[0]).name} <manifest-path> <registry/repo:tag> [changelog-path]",
        file=sys.stderr,
    )
    return 1


def manifest_path(base: Path, entry: dict) -> Path:
    return (base / entry["file"]).resolve()


def main() -> int:
    if len(sys.argv) not in (3, 4):
        return usage()

    manifest_file = Path(sys.argv[1]).resolve()
    reference = sys.argv[2]
    changelog_file = Path(sys.argv[3]).resolve() if len(sys.argv) == 4 else None

    with manifest_file.open("r", encoding="utf-8") as f:
        manifest = json.load(f)

    base_dir = manifest_file.parent
    kernel = manifest_path(base_dir, manifest["kernel"])
    root = manifest_path(base_dir, manifest["root"])
    verity = manifest_path(base_dir, manifest["verity"])

    cmd = [
        "oras",
        "push",
        "--disable-path-validation",
        "--config",
        f"{manifest_file}:{MANIFEST_MEDIA_TYPE}",
        "--artifact-type",
        MANIFEST_MEDIA_TYPE,
        reference,
        f"{kernel}:{UKI_MEDIA_TYPE}",
        f"{root}:{ROOT_MEDIA_TYPE}",
        f"{verity}:{VERITY_MEDIA_TYPE}",
    ]

    if changelog_file is not None:
        cmd.append(f"{changelog_file}:{CHANGELOG_MEDIA_TYPE}")

    completed = subprocess.run(cmd, check=False)
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
