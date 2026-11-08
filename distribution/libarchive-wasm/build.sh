#!/usr/bin/env bash
# Builds the comic reader's libarchive.wasm and worker-bundle.js without any
# crypto library. See README.md.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
work="${MEDIAFLICK_LIBARCHIVE_WASM_WORK_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/mediaflick/libarchive-wasm}"
jobs="${MEDIAFLICK_LIBARCHIVE_WASM_JOBS:-$(nproc)}"
mode="${1:-check}"
case "$mode" in check | update) ;; *) echo "usage: build.sh [check|update]" >&2; exit 2 ;; esac

EMSDK_VERSION=6.0.13
EMSDK_COMMIT=35659964bc2762fffec0d257997fb61475f765b7
EMSCRIPTEN_COMMIT=e13ee988ce3d87a738dd8f3661d02223308c412a

# name|version|url|sha256
SOURCES=(
  "libarchive|3.8.11|https://github.com/libarchive/libarchive/releases/download/v3.8.11/libarchive-3.8.11.tar.xz|a3dc0872b1b39da34dc18ef1b944f0f11ffd37425d70ae04be679279f1cf43d2"
  "zlib|1.3.2|https://github.com/madler/zlib/releases/download/v1.3.2/zlib-1.3.2.tar.xz|d7a0654783a4da529d1bb793b7ad9c3318020af77667bcae35f95d0e42a792f3"
  "bzip2|1.0.8|https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz|ab5a03176ee106d3f0fa90e381da478ddae405918153cca248e682cd0c4a2269"
  "xz|5.8.4|https://github.com/tukaani-project/xz/releases/download/v5.8.4/xz-5.8.4.tar.xz|4ce24038fd4221e0d13bc1a2de7a4db56e90b92b3bf75321f6c14be73f65de4b"
  "libarchivejs|2.0.2|https://github.com/nika-begiashvili/libarchivejs/archive/refs/tags/v2.0.2.tar.gz|4b7ded40d85bfb5787952828695033603aabdcd7b2ac9606281290290e8abb0f"
)

downloads="$work/downloads"
src="$work/src"
build="$work/build"
prefix="$work/prefix"
staged="$work/bundle"
mkdir -p "$downloads"

for entry in "${SOURCES[@]}"; do
  IFS='|' read -r name version url sha <<<"$entry"
  archive="$downloads/$name-$version.tarball"
  if [[ ! -f "$archive" ]]; then
    curl -fsSL --retry 3 -o "$archive.part" "$url"
    mv "$archive.part" "$archive"
  fi
  echo "$sha  $archive" | sha256sum --check --quiet
done

emsdk="$work/emsdk"
[[ -d "$emsdk/.git" ]] || git clone --quiet https://github.com/emscripten-core/emsdk.git "$emsdk"
git -C "$emsdk" fetch --quiet --tags origin
git -C "$emsdk" checkout --quiet "$EMSDK_COMMIT"
[[ "$(git -C "$emsdk" rev-parse "$EMSDK_VERSION^{commit}")" == "$EMSDK_COMMIT" ]] ||
  { echo "emsdk tag $EMSDK_VERSION no longer points at $EMSDK_COMMIT" >&2; exit 1; }
"$emsdk/emsdk" install "$EMSDK_VERSION" >/dev/null
"$emsdk/emsdk" activate "$EMSDK_VERSION" >/dev/null
# shellcheck disable=SC1091
EMSDK_QUIET=1 source "$emsdk/emsdk_env.sh"
emcc --version | head -n1 | grep -q "($EMSCRIPTEN_COMMIT)" ||
  { echo "emcc is not Emscripten $EMSCRIPTEN_COMMIT" >&2; exit 1; }

rm -rf "$src" "$build" "$prefix" "$staged"
mkdir -p "$src" "$build" "$prefix/include" "$prefix/lib" "$staged"
for entry in "${SOURCES[@]}"; do
  IFS='|' read -r name version url _ <<<"$entry"
  mkdir -p "$src/$name"
  tar -xf "$downloads/$name-$version.tarball" -C "$src/$name" --strip-components=1
done

# Paths in __FILE__ and assertions would otherwise carry this machine's work directory.
cflags=(-O3 "-ffile-prefix-map=$work=.")
export CFLAGS="${cflags[*]}"
cmake_common=(-G "Unix Makefiles" -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix"
  -DCMAKE_FIND_ROOT_PATH="$prefix" -DBUILD_SHARED_LIBS=OFF -DCMAKE_C_FLAGS_RELEASE="")

emcmake cmake -S "$src/zlib" -B "$build/zlib" "${cmake_common[@]}" \
  -DZLIB_BUILD_SHARED=OFF -DZLIB_BUILD_TESTING=OFF >/dev/null
cmake --build "$build/zlib" -j "$jobs" >/dev/null
cmake --install "$build/zlib" >/dev/null

bz="$src/bzip2"
for file in blocksort huffman crctable randtable compress decompress bzlib; do
  emcc "${cflags[@]}" -D_FILE_OFFSET_BITS=64 -c "$bz/$file.c" -o "$build/bz-$file.o"
done
emar rcs "$prefix/lib/libbz2.a" "$build"/bz-*.o
cp "$bz/bzlib.h" "$prefix/include/"

emcmake cmake -S "$src/xz" -B "$build/xz" "${cmake_common[@]}" \
  -DXZ_THREADS=no -DXZ_NLS=OFF -DXZ_DOC=OFF -DXZ_TOOL_XZ=OFF -DXZ_TOOL_XZDEC=OFF \
  -DXZ_TOOL_LZMADEC=OFF -DXZ_TOOL_LZMAINFO=OFF -DXZ_TOOL_SCRIPTS=OFF >/dev/null
cmake --build "$build/xz" -j "$jobs" >/dev/null
cmake --install "$build/xz" >/dev/null

# Every crypto and optional library is switched off by name. RAR5's BLAKE2sp
# is libarchive's own (CC0) implementation.
emcmake cmake -S "$src/libarchive" -B "$build/libarchive" "${cmake_common[@]}" \
  -DENABLE_OPENSSL=OFF -DENABLE_MBEDTLS=OFF -DENABLE_NETTLE=OFF -DENABLE_CNG=OFF \
  -DENABLE_MD=OFF -DENABLE_LIBB2=OFF -DENABLE_LZ4=OFF -DENABLE_LZO=OFF -DENABLE_ZSTD=OFF \
  -DENABLE_LIBXML2=OFF -DENABLE_BSDXML=OFF -DENABLE_EXPAT=OFF -DENABLE_WIN32_XMLLITE=OFF \
  -DENABLE_PCREPOSIX=OFF -DENABLE_PCRE2POSIX=OFF -DENABLE_LIBGCC=OFF \
  -DENABLE_ZLIB=ON -DENABLE_BZip2=ON -DENABLE_LZMA=ON -DENABLE_ICONV=ON \
  -DENABLE_XATTR=OFF -DENABLE_ACL=OFF -DENABLE_TAR=OFF -DENABLE_CPIO=OFF -DENABLE_CAT=OFF \
  -DENABLE_UNZIP=OFF -DENABLE_TEST=OFF -DENABLE_WERROR=OFF >"$build/libarchive-configure.log"
# A codec libarchive can't find silently falls back to "unsupported" (7z's LZMA among them).
for header in ZLIB_H BZLIB_H LZMA_H; do
  grep -q "^#define HAVE_$header 1" "$build/libarchive/config.h" || { echo "libarchive did not find $header" >&2; exit 1; }
done
cmake --build "$build/libarchive" -j "$jobs" >/dev/null
cmake --install "$build/libarchive" >/dev/null

mkdir -p "$build/glue"
# libarchive.js 2.0.2's worker was written against Emscripten 3.1: it reads
# cwrap and HEAPU8 off the module and does arithmetic on int64 returns such as
# archive_entry_size, which WASM_BIGINT (the default since) turns into BigInts.
emcc "${cflags[@]}" -I"$prefix/include" -c "$src/libarchivejs/lib/wrapper/main.c" -o "$build/main.o"
emcc "$build/main.o" "$prefix/lib/libarchive.a" "$prefix/lib/liblzma.a" "$prefix/lib/libbz2.a" "$prefix/lib/libz.a" \
  -o "$build/glue/libarchive.js" -O3 \
  -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=libarchive -sENVIRONMENT=worker -sALLOW_MEMORY_GROWTH=1 -sWASM_BIGINT=0 \
  -sEXPORTED_RUNTIME_METHODS=cwrap,HEAPU8 -sEXPORTED_FUNCTIONS=@"$src/libarchivejs/lib/tools/lib.exports"

cp -r "$src/libarchivejs/src/webworker" "$build/worker"
cp "$build/glue/libarchive.js" "$build/worker/wasm-gen/libarchive.js"
node "$here/bundle-worker.mjs" "$repo/ui" "$build/worker/browser-worker.js" "$staged/worker-bundle.js"
install -m 0644 "$build/glue/libarchive.wasm" "$staged/libarchive.wasm"

if strings -n 6 "$staged/libarchive.wasm" | grep -E -i -e 'openssl|libcrypto|EVP_|mbedtls|nettle' -e "$work"; then
  echo "libarchive.wasm still carries crypto symbols or build paths (above)" >&2
  exit 1
fi

(cd "$staged" && sha256sum libarchive.wasm worker-bundle.js) >"$work/SHA256SUMS"
if [[ "$mode" == update ]]; then
  mkdir -p "$here/bundle"
  cp "$staged/libarchive.wasm" "$staged/worker-bundle.js" "$here/bundle/"
  cp "$work/SHA256SUMS" "$here/SHA256SUMS"
  echo "updated $here/bundle"
elif ! diff -u "$here/SHA256SUMS" "$work/SHA256SUMS"; then
  echo "the rebuild differs from the committed bundle (above); run build.sh update to replace it" >&2
  exit 1
else
  (cd "$here/bundle" && sha256sum --check --quiet "$here/SHA256SUMS")
  echo "rebuild matches the committed bundle"
fi
cat "$work/SHA256SUMS"
