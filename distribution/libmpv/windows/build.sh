#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
winbuild_commit="cd1edc11dc6887a50f705717619d879f5a93a488"
patch_file="$script_dir/mpv-winbuild-cmake.patch"
vapoursynth_runtime_patch="$script_dir/mpv-vapoursynth-runtime.patch"
cache_root="${XDG_CACHE_HOME:-$HOME/.cache}/mediaflick"
work_dir="${MEDIAFLICK_LIBMPV_WORK_DIR:-$cache_root/libmpv-windows-x64}"
output_dir="${1:-$repo_root/build/libmpv-windows-x64}"
winbuild_dir="$work_dir/mpv-winbuild-cmake"
source_cache="$work_dir/sources"
rustup_cache="$work_dir/rustup"
build_dir="$work_dir/build"

for command_name in git cmake ninja sha256sum tar zstd; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "Missing required build command: $command_name" >&2
        exit 1
    fi
done

mkdir -p "$work_dir" "$source_cache" "$rustup_cache" "$output_dir"

if [[ ! -d "$winbuild_dir/.git" ]]; then
    if [[ -e "$winbuild_dir" ]] && find "$winbuild_dir" -mindepth 1 -print -quit | grep -q .; then
        echo "Refusing to replace non-empty directory: $winbuild_dir" >&2
        exit 1
    fi
    git clone https://github.com/shinchiro/mpv-winbuild-cmake.git "$winbuild_dir"
    git -C "$winbuild_dir" checkout --detach "$winbuild_commit"
fi

actual_commit="$(git -C "$winbuild_dir" rev-parse HEAD)"
if [[ "$actual_commit" != "$winbuild_commit" ]]; then
    echo "Unexpected mpv-winbuild-cmake revision: $actual_commit" >&2
    echo "Expected: $winbuild_commit" >&2
    exit 1
fi

git -C "$winbuild_dir" reset --hard "$winbuild_commit"
git -C "$winbuild_dir" clean -fd
git -C "$winbuild_dir" apply "$patch_file"

rm -f "$winbuild_dir/mediaflick-vapoursynth-runtime.patch"
install -m 0644 \
    "$vapoursynth_runtime_patch" \
    "$winbuild_dir/packages/mediaflick-vapoursynth-runtime.patch"

cmake \
    -S "$winbuild_dir" \
    -B "$build_dir" \
    -G Ninja \
    -DTARGET_ARCH=x86_64-w64-mingw32 \
    -DGCC_ARCH=x86-64 \
    -DSINGLE_SOURCE_LOCATION="$source_cache" \
    -DRUSTUP_LOCATION="$rustup_cache"

# The dependency graph assumes the cross compiler exists before any package is
# configured, so these targets must remain separate.
ninja -C "$build_dir" gcc
ninja -C "$build_dir" mpv

package_dir="$build_dir/mediaflick-libmpv-x86_64"
if [[ ! -f "$package_dir/libmpv-2.dll" ]]; then
    echo "The build completed without producing libmpv-2.dll." >&2
    exit 1
fi

install -m 0644 "$package_dir/libmpv-2.dll" "$output_dir/libmpv-2.dll"
install -m 0644 "$package_dir/LICENSE.GPL" "$output_dir/LICENSE.mpv-GPL"
install -m 0644 "$package_dir/LICENSE.LGPL" "$output_dir/LICENSE.mpv-LGPL"
install -m 0644 "$package_dir/Copyright" "$output_dir/Copyright.mpv"
install -m 0644 \
    "$script_dir/THIRD-PARTY-NOTICES.md" \
    "$output_dir/THIRD-PARTY-NOTICES.md"

manifest="$output_dir/SOURCE-REVISIONS.txt"
{
    printf 'MediaFlick Windows libmpv build\n'
    printf 'mpv-winbuild-cmake %s\n' "$winbuild_commit"
    printf 'MediaFlick build patch SHA-256 %s\n' "$(sha256sum "$patch_file" | cut -d' ' -f1)"
    printf 'MediaFlick VapourSynth runtime patch SHA-256 %s\n\n' \
        "$(sha256sum "$vapoursynth_runtime_patch" | cut -d' ' -f1)"
    printf 'Source repositories present in this build cache and source archive:\n'
    find "$source_cache" -mindepth 1 -maxdepth 1 -type d -print0 \
        | sort -z \
        | while IFS= read -r -d '' source_dir; do
            if git -C "$source_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
                remote="$(git -C "$source_dir" remote get-url origin 2>/dev/null || printf 'unknown')"
                revision="$(git -C "$source_dir" rev-parse HEAD)"
                printf '%s %s %s\n' "$(basename "$source_dir")" "$revision" "$remote"
            fi
        done
} > "$manifest"

dll_sha256="$(sha256sum "$output_dir/libmpv-2.dll" | cut -d' ' -f1)"
printf '%s  libmpv-2.dll\n' "$dll_sha256" > "$output_dir/libmpv-2.dll.sha256"

source_archive="$output_dir/mediaflick-libmpv-sources.tar.zst"
tar \
    --zstd \
    --exclude-vcs \
    --create \
    --file "$source_archive" \
    --directory "$work_dir" \
    mpv-winbuild-cmake sources

printf 'Built %s\n' "$output_dir/libmpv-2.dll"
printf 'Recorded corresponding sources in %s\n' "$source_archive"
