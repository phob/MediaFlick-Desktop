#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="${VERSION:-$(python3 - <<'PY'
import re
from pathlib import Path
text = Path('Cargo.toml').read_text(encoding='utf-8')
package = re.search(r'(?ms)^\[package\]\s*(.*?)(?:^\[|\Z)', text)
version = re.search(r'(?m)^version\s*=\s*"([^"]+)"', package.group(1) if package else '')
if not version:
    raise SystemExit('Cargo.toml [package] version not found')
print(version.group(1))
PY
)}"
tag="${TAG:-v${version}}"
arch="${MACOS_ARTIFACT_ARCH:-$(uname -m)}"
case "$arch" in
    amd64) arch="x86_64" ;;
    arm64) arch="arm64" ;;
    aarch64) arch="arm64" ;;
esac

target_dir="${CARGO_TARGET_DIR:-build/cargo-target}/release"
binary="$target_dir/mediaflick-desktop"
if [[ ! -x "$binary" ]]; then
    echo "Missing release binary: $binary" >&2
    echo "Run 'cargo build --release --bin mediaflick-desktop' first." >&2
    exit 1
fi

cef_search_roots=()
if [[ -n "${CEF_PATH:-}" ]]; then
    cef_search_roots+=("$CEF_PATH")
fi
cef_search_roots+=("$target_dir" ".cache/cef")

cef_framework=""
for root in "${cef_search_roots[@]}"; do
    if [[ -d "$root" ]]; then
        match="$(find "$root" -name 'Chromium Embedded Framework.framework' -type d -print -quit 2>/dev/null || true)"
        if [[ -n "$match" ]]; then
            cef_framework="$match"
            break
        fi
    fi
done

if [[ -z "$cef_framework" ]]; then
    echo "Could not locate Chromium Embedded Framework.framework. Set CEF_PATH to the CEF cache/root." >&2
    exit 1
fi

app_name="MediaFlick Desktop.app"
app="dist/macos/$app_name"
contents="$app/Contents"
macos_dir="$contents/MacOS"
frameworks_dir="$contents/Frameworks"
resources_dir="$contents/Resources"
rm -rf "$app" "dist/dmg"
mkdir -p "$macos_dir" "$frameworks_dir" "$resources_dir" "dist/dmg"

install -m 0755 "$binary" "$macos_dir/mediaflick-desktop"
cp -R "$cef_framework" "$frameworks_dir/"
cp resources/macos/AppIcon.icns "$resources_dir/AppIcon.icns"

python3 - "$version" <<'PY'
import sys
from pathlib import Path
version = sys.argv[1]
src = Path('resources/macos/Info.plist.in')
dst = Path('dist/macos/MediaFlick Desktop.app/Contents/Info.plist')
dst.write_text(src.read_text(encoding='utf-8').replace('@APP_VERSION_FULL@', version), encoding='utf-8')
PY

# CEF on macOS normally resolves resources from the app bundle Resources directory.
# Depending on the CEF archive layout, these may live next to the framework, in a
# sibling Resources directory, or inside the framework itself. Copy whatever is present.
resource_sources=(
    "$(dirname "$cef_framework")/Resources"
    "$(dirname "$(dirname "$cef_framework")")/Resources"
    "$cef_framework/Resources"
)
for src in "${resource_sources[@]}"; do
    if [[ -d "$src" ]]; then
        shopt -s nullglob
        for file in "$src"/*.pak "$src"/*.dat "$src"/*.bin; do
            cp -P "$file" "$resources_dir/"
        done
        shopt -u nullglob
        if [[ -d "$src/locales" ]]; then
            rm -rf "$resources_dir/locales"
            cp -R "$src/locales" "$resources_dir/locales"
        fi
    fi
done

# Some cef-dll-sys layouts keep the pack files in the framework parent rather than Resources/.
shopt -s nullglob
for file in "$(dirname "$cef_framework")"/*.pak "$(dirname "$cef_framework")"/*.dat "$(dirname "$cef_framework")"/*.bin; do
    cp -P "$file" "$resources_dir/"
done
shopt -u nullglob

# Ad-hoc sign the unsigned bundle so macOS has code signatures for nested CEF content.
codesign --force --deep --sign - "$app"

dmg_root="dist/dmg/root"
rm -rf "$dmg_root"
mkdir -p "$dmg_root"
cp -R "$app" "$dmg_root/$app_name"
ln -s /Applications "$dmg_root/Applications"

output="dist/MediaFlickDesktop-${tag}-macos-${arch}.dmg"
rm -f "$output"
hdiutil create \
    -volname "MediaFlick Desktop ${tag}" \
    -srcfolder "$dmg_root" \
    -ov \
    -format UDZO \
    "$output"

codesign --force --sign - "$output" || true

echo "DMG: $output"
