#!/usr/bin/env python3
"""Build MediaFlick Companion release metadata and its Jellyfin catalog entry."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import zipfile
from datetime import datetime, timezone
from pathlib import Path


GUID = "11d8f2bb-2b9d-4ce1-8c33-5a0f809dfd2f"
TARGET_ABI = "10.11.11.0"
PLUGIN_NAME = "MediaFlick Companion"
DESCRIPTION = (
    "Secure Jellyfin-authenticated calendar and Seerr gateway for MediaFlick clients."
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--publish-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--source-url", required=True)
    args = parser.parse_args()

    version = args.version.removeprefix("v")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?", version):
        parser.error("version must be a semantic version")

    assembly = args.publish_dir / "Jellyfin.Plugin.MediaFlick.dll"
    if not assembly.is_file():
        parser.error(f"published assembly not found: {assembly}")
    args.output_dir.mkdir(parents=True, exist_ok=True)

    timestamp = datetime.now(timezone.utc).replace(microsecond=0).isoformat()
    meta = {
        "category": "General",
        "changelog": "Initial companion API, calendar, and Seerr mediation.",
        "description": DESCRIPTION,
        "guid": GUID,
        "name": PLUGIN_NAME,
        "overview": DESCRIPTION,
        "owner": "phob",
        "targetAbi": TARGET_ABI,
        "timestamp": timestamp,
        "version": version,
    }
    meta_path = args.output_dir / "meta.json"
    meta_path.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")

    zip_path = args.output_dir / f"mediaflick-companion_{version}.zip"
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.write(assembly, assembly.name)
        archive.write(meta_path, meta_path.name)

    checksum = hashlib.md5(zip_path.read_bytes()).hexdigest()
    manifest = [
        {
            "guid": GUID,
            "name": PLUGIN_NAME,
            "description": DESCRIPTION,
            "overview": DESCRIPTION,
            "owner": "phob",
            "category": "General",
            "versions": [
                {
                    "version": version,
                    "changelog": meta["changelog"],
                    "targetAbi": TARGET_ABI,
                    "sourceUrl": args.source_url,
                    "checksum": checksum,
                    "timestamp": timestamp,
                }
            ],
        }
    ]
    (args.output_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
