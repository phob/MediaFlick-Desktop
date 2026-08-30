# MediaFlick Windows libmpv build

MediaFlick ships a dedicated 64-bit `libmpv-2.dll` on Windows. It is loaded at
runtime, so users can still select their own external mpv or MPC-HC and the app
can report a missing or incompatible bundled library without failing to start.

The build is based on a pinned `mpv-winbuild-cmake` revision plus the adjacent
patch. It produces libmpv without the mpv command-line player, JavaScript,
Vulkan, optical-disc, archive, Rubber Band, or external encoding libraries. It
includes Lua and VapourSynth support for SVP compatibility. VapourSynth itself
is not bundled, so an installed SVP or VapourSynth runtime must provide
`VSScript.dll` and its plugin dependencies. MediaFlick's mpv patch loads that
DLL only when the VapourSynth filter starts, which keeps ordinary libmpv
playback working when VapourSynth is not installed. The build keeps FFmpeg
network playback, Windows Schannel TLS, D3D11/D3D11VA, OpenGL, libass subtitle
rendering, ICC color management, and common software decoders. The baseline is
x86-64 rather than x86-64-v3.

mpv 0.41.0 and FFmpeg 8.0.1 are pinned in the patch. GPL-only mpv and FFmpeg
features are disabled. Every build records the exact revisions of transitive
source repositories, license notices, a DLL checksum, and a corresponding
source archive.

## Build on Linux or WSL

Install the dependencies listed by `mpv-winbuild-cmake`. On Ubuntu that means
the cross-toolchain prerequisites from its README plus CMake, Ninja, Meson,
Git, Python with Jinja2, NASM, Yasm, Ragel, and zstd. Then run:

```sh
bash scripts/build-libmpv-windows.sh
```

The first build creates its own MinGW-w64/GCC toolchain and can take a while.
The reusable work tree defaults to `.cache/libmpv-windows-x64`; override it
with `MEDIAFLICK_LIBMPV_WORK_DIR`. Runtime and source-compliance artifacts are
written to `build/libmpv-windows-x64` unless an output directory is passed as
the first argument.

Do not publish the DLL without its license notices, `SOURCE-REVISIONS.txt`, and
the generated corresponding-source archive. This is a release-engineering
requirement, not legal advice.
