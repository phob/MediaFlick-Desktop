#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/mediaflick-libmpv.XXXXXXXX")"
staged_output="$staging_dir/output"
destination="$repo_root/build/libmpv-windows-x64"
incoming="$repo_root/build/.libmpv-windows-x64.incoming"
work_dir="${MEDIAFLICK_LIBMPV_WORK_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/mediaflick/libmpv-windows-x64}"

if bash "$script_dir/build.sh" "$staged_output"; then
    mkdir -p "$repo_root/build"
    rm -rf "$incoming"
    cp -a "$staged_output" "$incoming"
    rm -rf "$destination"
    mv "$incoming" "$destination"
    rm -rf "$staging_dir"
    printf 'Copied libmpv artifacts to %s\n' "$destination"
else
    status=$?
    rm -rf "$staging_dir"
    printf 'libmpv build failed; logs retained under %s/build\n' "$work_dir" >&2
    exit "$status"
fi
