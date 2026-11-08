# libarchive.wasm for the comic reader

The book reader opens CBR (RAR 4 and 5), CB7 and CBT comics with
[libarchive.js](https://github.com/nika-begiashvili/libarchivejs) 2.0.2. Its
npm package ships a `libarchive.wasm` that statically links OpenSSL 1.0.2s
libcrypto, whose license is incompatible with the GPL. MediaFlick embeds the
wasm in its GPL-2.0-or-later binary, so it ships this rebuild instead. Reading
RAR, 7z and tar needs no crypto library; only encrypted archives would.

`bundle/` holds the two files the reader loads in its worker, and
`SHA256SUMS` records their hashes. `ui/vite.config.ts` copies them to
`/reader/libarchive/` in place of the package's own copies. The package still
supplies `libarchive.js`, the main-thread API that starts the worker and talks
to it over Comlink. The worker is built from the same libarchive.js 2.0.2
sources, so the two halves match: `ui/vite.config.ts` fails the build, naming
both versions, when the package's version differs from the `libarchivejs` pin
in `build.sh`.

## What is linked

| Input | Version | License |
| --- | --- | --- |
| libarchive | 3.8.11 | BSD-2-Clause (some files BSD-3-Clause, public domain, or CC0) |
| zlib | 1.3.2 | zlib |
| bzip2 | 1.0.8 | bzip2 license (BSD-style) |
| xz (liblzma only) | 5.8.4 | 0BSD |
| libarchive.js wrapper and worker sources | 2.0.2 | MIT |
| Emscripten runtime, musl libc, compiler-rt, dlmalloc | emsdk 6.0.13 | MIT or NCSA, MIT, Apache-2.0 WITH LLVM-exception, CC0 |
| Comlink, from `ui/pnpm-lock.yaml` | 4.4.2 | Apache-2.0 |

libarchive is configured with OpenSSL, mbed TLS, Nettle, CNG, libmd, libb2,
LZ4, LZO, zstd, libxml2, expat and the PCRE regex libraries all switched off,
and the link line names only the four libraries above. RAR5's BLAKE2sp
checksums come from libarchive's own copy, which it offers under CC0.
libarchive.js 2.0.2's upstream build added no zstd either.

`build.sh` pins each source by URL and SHA-256 and the toolchain by emsdk tag
and commit. It bundles the worker with the rolldown that `ui/pnpm-lock.yaml`
pins for Vite, so run `pnpm --dir ui install` first.

## Rebuild and verify

On Linux with bash, coreutils, curl, git, CMake, make, Python 3 and Node:

```sh
pnpm --dir ui install
just libarchive-wasm          # rebuild and compare with bundle/
just libarchive-wasm update   # rebuild and replace bundle/ and SHA256SUMS
```

The first run downloads emsdk (about 1.7 GB) and the sources into
`$XDG_CACHE_HOME/mediaflick/libarchive-wasm` or
`~/.cache/mediaflick/libarchive-wasm`. Set `MEDIAFLICK_LIBARCHIVE_WASM_WORK_DIR`
to move it and `MEDIAFLICK_LIBARCHIVE_WASM_JOBS` to limit parallelism. A run
takes about ten minutes, most of it libarchive's CMake feature probes under
Emscripten. Newer CMake releases print a warning that Emscripten shared
libraries are unsupported; the build is static and unaffected.

Every run fails if a download's hash differs, if libarchive does not find
zlib, bzip2 or liblzma, or if the wasm contains an OpenSSL, libcrypto, EVP,
mbed TLS or Nettle string or the work directory's path. The default `check`
mode then fails unless the rebuilt files match `SHA256SUMS` byte for byte, and
checks that the committed `bundle/` matches it too. App builds never run this
script and never touch the network for it.

To move a pin, change it in `build.sh`, run `just libarchive-wasm update`,
and update the versions above and in `THIRD-PARTY-NOTICES.md`. Then open
the three archive comics in the real app:

```sh
MEDIAFLICK_VERIFY_BOOKS=cbt,cb7,cbr just verify books --account <your server>
```

A clean build can still fail in the reader. libarchive.js 2.0.2's worker
targets Emscripten 3.1, and newer releases changed two defaults it relies
on, which `build.sh` sets back. A rolldown or Comlink update in the UI
lockfile changes the bytes of `worker-bundle.js`; when `check` reports that,
run `update` and the drive again.
