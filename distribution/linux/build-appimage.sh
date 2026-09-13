#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
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
arch="${APPIMAGE_ARCH:-$(uname -m)}"
case "$arch" in
    amd64) arch="x86_64" ;;
    arm64) arch="aarch64" ;;
esac

libmpv_dir="${MEDIAFLICK_LIBMPV_DIR:-build/libmpv-linux-$arch}"
libmpv_sources="mediaflick-libmpv-linux-$arch-sources.tar.zst"
for file in lib/libmpv.so.2 SHA256SUMS SOURCE-REVISIONS.txt SYSTEM-PACKAGES.txt \
    HOST-LIBRARIES.txt MPV-CONFIG.h FFMPEG-CONFIG.h FFMPEG-CONFIG.mak \
    licenses/THIRD-PARTY-NOTICES.md "$libmpv_sources"; do
    if [[ ! -s "$libmpv_dir/$file" ]]; then
        echo "Missing required Linux libmpv artifact: $libmpv_dir/$file" >&2
        echo "Run 'just libmpv' first (see distribution/libmpv/linux/README.md)." >&2
        exit 1
    fi
done
(cd "$libmpv_dir" && sha256sum --check --strict SHA256SUMS)

target_dir="${CARGO_TARGET_DIR:-build/cargo-target}/release"
binary="$target_dir/mediaflick-desktop"
if [[ ! -x "$binary" ]]; then
    echo "Missing release binary: $binary" >&2
    echo "Run 'cargo build --release --bin mediaflick-desktop' first." >&2
    exit 1
fi

appdir="dist/linux/MediaFlickDesktop.AppDir"
rm -rf "$appdir"
mkdir -p \
    "$appdir/usr/bin" \
    "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/scalable/apps" \
    "$appdir/usr/share/metainfo" \
    "$appdir/usr/share/doc/mediaflick-desktop" \
    "dist/linux"

install -m 0755 "$binary" "$appdir/usr/bin/mediaflick-desktop"

# CEF's Linux runtime files are copied next to the Cargo binary by cef-dll-sys.
shopt -s nullglob
for pattern in \
    "*.so" "*.so.*" \
    "*.bin" "*.dat" "*.pak" \
    "chrome-sandbox" "snapshot_blob.bin" "v8_context_snapshot.bin" \
    "vk_swiftshader_icd.json"; do
    for file in "$target_dir"/$pattern; do
        if [[ -f "$file" ]]; then
            cp -P "$file" "$appdir/usr/bin/"
        fi
    done
done
shopt -u nullglob

# The loader already recognizes this private directory. Each library has an
# origin-relative RUNPATH, so codec dependencies need no global linker override.
# Never pick up a system/custom libmpv accidentally staged with the Cargo output.
rm -f "$appdir/usr/bin/"libmpv.so*
cp -R "$libmpv_dir/lib" "$appdir/usr/bin/libmpv"
libmpv_docs="$appdir/usr/share/doc/mediaflick-desktop/libmpv"
mkdir -p "$libmpv_docs"
cp -R "$libmpv_dir/licenses" "$libmpv_docs/"
cp "$libmpv_dir/"*.txt "$libmpv_dir/"*CONFIG* "$libmpv_dir/SHA256SUMS" "$libmpv_docs/"
python3 distribution/libmpv/linux/smoke-test.py "$appdir/usr/bin/libmpv/libmpv.so.2"
# Publish corresponding sources alongside the AppImage, not inside its runtime.
cp "$libmpv_dir/$libmpv_sources" "dist/linux/$libmpv_sources"

required=(
    "$appdir/usr/bin/libcef.so"
    "$appdir/usr/bin/icudtl.dat"
    "$appdir/usr/bin/resources.pak"
    "$target_dir/CREDITS.html"
)
for file in "${required[@]}"; do
    if [[ ! -e "$file" ]]; then
        echo "Missing required CEF runtime file in AppDir: $file" >&2
        exit 1
    fi
done

if [[ ! -d "$target_dir/locales" ]]; then
    echo "Missing required CEF locales directory: $target_dir/locales" >&2
    exit 1
fi
cp -R "$target_dir/locales" "$appdir/usr/bin/locales"

install -m 0644 LICENSE THIRD-PARTY-NOTICES.md "$target_dir/CREDITS.html" \
    "$appdir/usr/share/doc/mediaflick-desktop/"
cp distribution/linux/io.github.phob.MediaFlickDesktop.desktop "$appdir/usr/share/applications/io.github.phob.MediaFlickDesktop.desktop"
cp distribution/linux/io.github.phob.MediaFlickDesktop.metainfo.xml "$appdir/usr/share/metainfo/io.github.phob.MediaFlickDesktop.metainfo.xml"
cp distribution/app-icon.svg "$appdir/usr/share/icons/hicolor/scalable/apps/io.github.phob.MediaFlickDesktop.svg"
cp distribution/linux/io.github.phob.MediaFlickDesktop.desktop "$appdir/io.github.phob.MediaFlickDesktop.desktop"
cp distribution/app-icon.svg "$appdir/io.github.phob.MediaFlickDesktop.svg"

cat > "$appdir/AppRun" <<'SH'
#!/usr/bin/env sh
set -eu
here="$(dirname "$(readlink -f "$0")")"
cef_lib="$here/usr/bin/libcef.so"
export LD_LIBRARY_PATH="$here/usr/bin${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -f "$cef_lib" ]; then
    # CEF's Linux close(2) wrapper resolves the real libc symbol with
    # dlsym(RTLD_NEXT, "close"). Preloading libcef keeps it before libc in the
    # dynamic link map, avoiding CEF's startup-time "close symbol missing" abort
    # when the packaged binary's DT_NEEDED order places libc first.
    export MEDIAFLICK_DESKTOP_CEF_PRELOAD="$cef_lib"
    export LD_PRELOAD="$cef_lib${LD_PRELOAD:+ $LD_PRELOAD}"
fi
exec "$here/usr/bin/mediaflick-desktop" "$@"
SH
chmod +x "$appdir/AppRun"

appimagetool="${APPIMAGETOOL:-}"
if [[ -z "$appimagetool" ]]; then
    if command -v appimagetool >/dev/null 2>&1; then
        appimagetool="$(command -v appimagetool)"
    else
        mkdir -p build/tools
        appimagetool="build/tools/appimagetool-${arch}.AppImage"
        if [[ ! -x "$appimagetool" ]]; then
            curl -L --fail \
                -o "$appimagetool" \
                "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-${arch}.AppImage"
            chmod +x "$appimagetool"
        fi
    fi
fi

output="dist/linux/MediaFlickDesktop-${tag}-linux-${arch}.AppImage"
rm -f "$output"
ARCH="$arch" APPIMAGE_EXTRACT_AND_RUN=1 "$appimagetool" "$appdir" "$output"
chmod +x "$output"

echo "AppImage: $output"
