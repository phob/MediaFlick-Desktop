#!/usr/bin/env python3
"""Package the MediaFlick Companion and maintain its Jellyfin repository manifest.

  package.py build --version 0.2.2.0 --publish-dir DIR --output-dir DIR [--changelog TEXT]
      Zip a `dotnet publish` output with its meta.json, as Jellyfin installs it.

  package.py add --source-url URL --manifest website/public/companion/manifest.json
      Download the package at URL without credentials and add it to the manifest.
      The entry's version, targetAbi, changelog and timestamp come from the
      package's meta.json and its checksum from the downloaded bytes, so an
      entry only exists for a download Jellyfin can fetch. A draft release's
      asset is not public, so it cannot be added. Earlier versions stay listed
      and a listed version is never replaced.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import io
import re
import urllib.request
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from xml.etree import ElementTree


GUID = "11d8f2bb-2b9d-4ce1-8c33-5a0f809dfd2f"
PROJECT = Path(__file__).parent / "src" / "Jellyfin.Plugin.MediaFlick" / "Jellyfin.Plugin.MediaFlick.csproj"
PLUGIN_NAME = "MediaFlick Companion"
VERSION = re.compile(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:\.(?:0|[1-9]\d*))?")
DESCRIPTION = (
    "Calendar, requests, ratings, and collections for MediaFlick Desktop, with provider credentials kept on the server."
)


def target_abi() -> str:
    # Jellyfin reads targetAbi as the minimum server version. The build only
    # knows the APIs of the Jellyfin.Controller package it compiles against.
    for reference in ElementTree.parse(PROJECT).getroot().iter("PackageReference"):
        if reference.get("Include") == "Jellyfin.Controller":
            version = reference.get("Version", "")
            if not re.fullmatch(r"\d+\.\d+\.\d+", version):
                raise SystemExit(f"Jellyfin.Controller needs an exact three-part version, found {version!r}")
            return f"{version}.0"
    raise SystemExit(f"Jellyfin.Controller reference not found in {PROJECT}")


def version_key(version: str) -> tuple[int, int, int, int]:
    # Jellyfin compares System.Version values; 0.2.2 and 0.2.2.0 name the same build.
    parts = [int(part) for part in version.split(".")]
    return parts[0], parts[1], parts[2], parts[3] if len(parts) == 4 else 0


def add_version(catalog: list[dict], entry: dict) -> list[dict]:
    """The catalog with `entry` added, newest version first."""
    if not catalog:
        record = {
            "guid": GUID,
            "name": PLUGIN_NAME,
            "description": DESCRIPTION,
            "overview": DESCRIPTION,
            "owner": "phob",
            "category": "General",
            "versions": [],
        }
    elif len(catalog) == 1 and catalog[0].get("guid") == GUID and isinstance(catalog[0].get("versions"), list):
        record = catalog[0]
    else:
        raise SystemExit("the manifest must be empty or list only the MediaFlick Companion")
    key = version_key(entry["version"])
    if any(version_key(listed["version"]) == key for listed in record["versions"]):
        raise SystemExit(f"version {entry['version']} is already listed; publish a new version instead of replacing it")
    versions = sorted([*record["versions"], entry], key=lambda listed: version_key(listed["version"]), reverse=True)
    return [{**record, "versions": versions}]


def build(args: argparse.Namespace) -> None:
    version = args.version.removeprefix("v")
    if not VERSION.fullmatch(version):
        raise SystemExit("version must have three or four numeric components for Jellyfin")

    assembly = args.publish_dir / "Jellyfin.Plugin.MediaFlick.dll"
    if not assembly.is_file():
        raise SystemExit(f"published assembly not found: {assembly}")
    license_path = args.publish_dir / "LICENSE"
    if not license_path.is_file():
        raise SystemExit(f"published license not found: {license_path}")
    args.output_dir.mkdir(parents=True, exist_ok=True)

    meta = {
        "category": "General",
        "changelog": args.changelog,
        "description": DESCRIPTION,
        "guid": GUID,
        "name": PLUGIN_NAME,
        "overview": DESCRIPTION,
        "owner": "phob",
        "targetAbi": target_abi(),
        "timestamp": datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
        "version": version,
    }
    meta_path = args.output_dir / "meta.json"
    meta_path.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")

    zip_path = args.output_dir / f"mediaflick-companion_{version}.zip"
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.write(assembly, assembly.name)
        archive.write(meta_path, meta_path.name)
        archive.write(license_path, license_path.name)


def add(args: argparse.Namespace) -> None:
    with urllib.request.urlopen(args.source_url, timeout=120) as response:
        data = response.read()
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        meta = json.loads(archive.read("meta.json"))
    version = meta.get("version")
    if (
        meta.get("guid") != GUID
        or not isinstance(version, str)
        or not VERSION.fullmatch(version)
        or not all(isinstance(meta.get(field), str) for field in ("changelog", "targetAbi", "timestamp"))
    ):
        raise SystemExit("the package's meta.json is not a complete MediaFlick Companion meta.json")
    if not args.source_url.endswith(f"/mediaflick-companion_{version}.zip"):
        raise SystemExit(f"the package at {args.source_url} says it is version {version}")
    entry = {
        "version": version,
        "changelog": meta["changelog"],
        "targetAbi": meta["targetAbi"],
        "sourceUrl": args.source_url,
        "checksum": hashlib.md5(data).hexdigest(),
        "timestamp": meta["timestamp"],
    }
    catalog = json.loads(args.manifest.read_text(encoding="utf-8")) if args.manifest.exists() else []
    if not isinstance(catalog, list):
        raise SystemExit(f"{args.manifest} is not a Jellyfin repository manifest")
    updated = add_version(catalog, entry)
    args.manifest.write_text(json.dumps(updated, indent=2) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build", help="package a published plugin into a zip and meta.json")
    build_parser.add_argument("--version", required=True)
    build_parser.add_argument("--publish-dir", type=Path, required=True)
    build_parser.add_argument("--output-dir", type=Path, required=True)
    build_parser.add_argument("--changelog", default="MediaFlick Companion calendar, requests, ratings, and collections.")
    build_parser.set_defaults(run=build)
    add_parser = commands.add_parser("add", help="add a downloadable package to a repository manifest")
    add_parser.add_argument("--source-url", required=True)
    add_parser.add_argument("--manifest", type=Path, required=True)
    add_parser.set_defaults(run=add)
    args = parser.parse_args()
    args.run(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
