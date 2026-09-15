"""Stage a public Jellyfin test catalog without publishing a GitHub release."""

import argparse
import json
import shutil
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-dir", required=True, type=Path)
    parser.add_argument("--repository-dir", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()
    manifest = json.loads((args.package_dir / "manifest.json").read_text(encoding="utf-8"))
    plugin = manifest[0]
    release = plugin["versions"][0]
    version = release["version"]
    # This tool only consumes the package tool's numeric Jellyfin versions.
    if len(version.split(".")) not in (3, 4) or not all(part.isascii() and part.isdigit() for part in version.split(".")):
        parser.error("invalid package version")
    destination = args.repository_dir / "packages" / version
    if destination.exists():
        parser.error("test package already exists; use a new version instead of replacing an installed build")
    catalog = args.repository_dir / "manifest.json"
    if catalog.exists():
        previous = json.loads(catalog.read_text(encoding="utf-8"))
        if len(previous) != 1 or previous[0]["guid"] != plugin["guid"]:
            parser.error("existing catalog belongs to a different plugin")
        plugin["versions"].extend(previous[0]["versions"])
        plugin["versions"].sort(key=lambda entry: tuple(int(part) for part in entry["version"].split(".")), reverse=True)
    destination.mkdir(parents=True)
    shutil.copy2(args.package_dir / f"mediaflick-companion_{version}.zip", destination)
    catalog.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (args.repository_dir / "README.md").write_text(
        "# MediaFlick Companion test repository\n\n"
        "Opt-in test packages for Jellyfin 12. These are not stable releases.\n\n"
        "Add this branch's raw `manifest.json` URL under Dashboard → Plugins → Repositories. "
        "Install MediaFlick Companion from the catalog, then restart Jellyfin. "
        "Desktop discovers it under Settings → MediaFlick Companion.\n\n"
        "This is the same plugin identity as the normal Companion; installing it updates an existing installation. "
        "Remove this repository when finished testing to stop receiving test updates.\n\n"
        f"Latest packaged source commit: `{args.source_sha}`.\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
