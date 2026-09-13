#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
arch="$(uname -m)"
case "$arch" in
    x86_64) cpu_flags="-march=x86-64 -mtune=generic" ;;
    aarch64) cpu_flags="-march=armv8-a" ;;
    *) echo "Unsupported Linux architecture: $arch" >&2; exit 1 ;;
esac
cache_root="${XDG_CACHE_HOME:-$HOME/.cache}/mediaflick"
work_dir="${MEDIAFLICK_LIBMPV_WORK_DIR:-$cache_root/libmpv-linux-$arch}"
output_dir="${1:-$repo_root/build/libmpv-linux-$arch}"
jobs="${MEDIAFLICK_LIBMPV_JOBS:-$(nproc)}"

for command_name in git meson ninja make cc c++ pkg-config nasm python3 patchelf strip tar zstd dpkg-query apt-get; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "Missing required build command: $command_name (see $script_dir/README.md)" >&2
        exit 1
    fi
done
pkg-config --exists libass lcms2 dav1d gnutls zlib alsa libpulse egl gl libva libva-x11 x11 xext xpresent xrandr xscrnsaver vulkan glslang ffnvcodec

mkdir -p "$work_dir" "$output_dir"
work_dir="$(realpath "$work_dir")"
output_dir="$(realpath "$output_dir")"
sources="$work_dir/sources"
prefix="$work_dir/prefix"
mkdir -p "$sources" "$prefix"

checkout() {
    local name="$1" url="$2" revision="$3"
    local source_patch="${4:-}"
    local dest="$sources/$name"
    if [[ ! -d "$dest" ]]; then
        git init "$dest"
        git -C "$dest" remote add origin "$url"
        git -C "$dest" fetch --depth=1 origin "$revision"
        git -C "$dest" checkout --detach FETCH_HEAD
    fi
    # Remove only our previously applied patch before checking source integrity.
    # Unrelated local changes still fail the check and are never reset away.
    if [[ -n "$source_patch" ]] && git -C "$dest" apply --reverse --check "$source_patch" 2>/dev/null; then
        git -C "$dest" apply --reverse "$source_patch"
    fi
    if [[ "$(git -C "$dest" rev-parse HEAD)" != "$revision" ]] ||
        [[ -n "$(git -C "$dest" status --porcelain --untracked-files=no)" ]]; then
        echo "Unexpected or modified source tree: $dest; use a fresh work directory." >&2
        exit 1
    fi
    if [[ -n "$source_patch" ]]; then
        git -C "$dest" apply "$source_patch"
    fi
}

# Match the Windows mpv and FFmpeg releases, with a Linux-only feature profile.
checkout mpv https://github.com/mpv-player/mpv.git 41f6a645068483470267271e1d09966ca3b9f413
checkout ffmpeg https://github.com/FFmpeg/FFmpeg.git 894da5ca7d742e4429ffb2af534fcda0103ef593
checkout libplacebo https://github.com/haasn/libplacebo.git 3188549fba13bbdf3a5a98de2a38c2e71f04e21e \
    "$script_dir/libplacebo-python314.patch"
git -C "$sources/libplacebo" submodule update --init --depth=1 \
    3rdparty/fast_float 3rdparty/glad 3rdparty/jinja 3rdparty/markupsafe

export PKG_CONFIG_PATH="$prefix/lib/pkgconfig"
export CFLAGS="$cpu_flags -O2 -fPIC"
export CXXFLAGS="$CFLAGS"

# This prefix belongs to the build. Reinstall it so removed features/libraries
# cannot survive a profile change in the reusable cache.
rm -rf "$prefix"
mkdir -p "$prefix" "$work_dir/ffmpeg-build"
cd "$work_dir/ffmpeg-build"
"$sources/ffmpeg/configure" \
    --prefix="$prefix" --libdir="$prefix/lib" \
    --disable-autodetect --disable-gpl --disable-nonfree \
    --disable-doc --disable-programs --disable-avdevice --disable-debug \
    --disable-static --enable-shared --enable-pic --enable-runtime-cpudetect \
    --disable-encoders --enable-encoder=ac3,png,mjpeg \
    --enable-libdav1d --enable-gnutls --enable-zlib --enable-vaapi \
    --enable-ffnvcodec --enable-nvdec --disable-nvenc \
    --extra-cflags="$CFLAGS"
make -j "$jobs"
make install

# Preserve the embedded player's Vulkan renderer and OpenGL fallback.
# glslang supplies Vulkan's SPIR-V compilation.
meson setup --wipe "$work_dir/placebo-build" "$sources/libplacebo" \
    --prefix="$prefix" --libdir=lib --buildtype=release --default-library=shared \
    --wrap-mode=nodownload -Dauto_features=disabled -Ddemos=false \
    -Dlcms=enabled -Ddovi=enabled -Dopengl=enabled -Dvulkan=enabled \
    -Dvk-proc-addr=enabled -Dglslang=enabled
ninja -C "$work_dir/placebo-build" -j "$jobs"
meson install -C "$work_dir/placebo-build" --strip

# Upstream mpv's X11 support requires its GPL build; FFmpeg stays LGPL-only.
meson setup --wipe "$work_dir/mpv-build" "$sources/mpv" \
    --prefix="$prefix" --libdir=lib --buildtype=release --default-library=shared \
    --wrap-mode=nodownload -Dauto_features=disabled \
    -Dlibmpv=true -Dcplayer=false -Dgpl=true -Dbuild-date=false -Db_lundef=true \
    -Dlua=disabled -Djavascript=disabled -Dvapoursynth=disabled \
    -Ddvdnav=disabled -Dlibbluray=disabled -Dcdda=disabled \
    -Dgl=enabled -Dplain-gl=enabled -Degl=enabled -Degl-x11=enabled -Dx11=enabled \
    -Dvulkan=enabled -Dvaapi=enabled -Dvaapi-x11=enabled \
    -Dcuda-hwaccel=enabled -Dcuda-interop=enabled \
    -Dalsa=enabled -Dpulse=enabled -Dlcms2=enabled -Diconv=enabled \
    -Dzlib=enabled -Dvector=enabled
ninja -C "$work_dir/mpv-build" -j "$jobs"
meson install -C "$work_dir/mpv-build" --strip

# Header-only and statically linked build dependencies are invisible to ldd,
# but their corresponding sources/notices belong in the payload too.
python3 "$script_dir/bundle.py" "$prefix" "$output_dir" "$sources/debian" \
    glslang-dev spirv-tools libffmpeg-nvenc-dev libvulkan-dev
cp "$script_dir/THIRD-PARTY-NOTICES.md" "$output_dir/licenses/"
for project in mpv ffmpeg libplacebo; do
    mkdir -p "$output_dir/licenses/$project"
    find "$sources/$project" -maxdepth 1 -type f \
        \( -name 'LICENSE*' -o -name 'COPYING*' -o -name 'Copyright' \) \
        -exec cp {} "$output_dir/licenses/$project/" \;
done
for project in fast_float glad jinja markupsafe; do
    mkdir -p "$output_dir/licenses/$project"
    cp "$sources/libplacebo/3rdparty/$project"/LICENSE* "$output_dir/licenses/$project/"
done
{
    printf 'MediaFlick Linux libmpv (%s)\n' "$arch"
    for project in mpv ffmpeg libplacebo; do
        printf '%s %s\n' "$project" "$(git -C "$sources/$project" rev-parse HEAD)"
    done
    git -C "$sources/libplacebo" submodule status
    printf '\nMediaFlick source patches (SHA-256):\n'
    sha256sum "$script_dir/"*.patch
    printf '\nBuild system:\n'
    cat /etc/os-release
    cc --version
    meson --version
} > "$output_dir/SOURCE-REVISIONS.txt"
cp "$work_dir/mpv-build/config.h" "$output_dir/MPV-CONFIG.h"
cp "$work_dir/placebo-build/src/include/libplacebo/config.h" "$output_dir/LIBPLACEBO-CONFIG.h"
cp "$work_dir/ffmpeg-build/config.h" "$output_dir/FFMPEG-CONFIG.h"
cp "$work_dir/ffmpeg-build/ffbuild/config.mak" "$output_dir/FFMPEG-CONFIG.mak"

# Include the build recipe and configuration as well as exact upstream and
# distribution sources for every redistributed dependency.
mkdir -p "$work_dir/recipe"
cp "$script_dir"/*.sh "$script_dir"/*.py "$script_dir"/*.md "$script_dir"/*.patch "$work_dir/recipe/"
cp "$output_dir/"*CONFIG* "$output_dir/SOURCE-REVISIONS.txt" \
    "$output_dir/SYSTEM-PACKAGES.txt" "$work_dir/recipe/"
tar --zstd --exclude-vcs -cf "$output_dir/mediaflick-libmpv-linux-$arch-sources.tar.zst" \
    -C "$work_dir" sources recipe
(cd "$output_dir" && sha256sum lib/*.so* > SHA256SUMS)
python3 "$script_dir/smoke-test.py" "$output_dir/lib/libmpv.so.2" "$output_dir/LIBPLACEBO-CONFIG.h"
printf 'Built Linux libmpv payload: %s\n' "$output_dir"
