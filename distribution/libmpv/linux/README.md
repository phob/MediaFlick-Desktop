# MediaFlick Linux libmpv build

Linux AppImages bundle a dedicated `libmpv.so.2`, built from the same mpv 0.41.0
and FFmpeg 8.0.1 revisions as Windows. The Linux profile keeps:

- Common software video/audio decoders, including H.264, HEVC, VP9, AV1 through
  dav1d, AAC, AC-3, DTS, and FLAC; normal containers and network protocols.
- HTTPS through GnuTLS, libass subtitles and Fontconfig font discovery,
  Little CMS color management, and mpv's OpenGL render API.
- X11/EGL rendering (including XWayland), VA-API decoding/interoperation, and
  ALSA/PulseAudio output. PipeWire desktops use their PulseAudio compatibility
  service. GPUs without usable VA-API can use software decoding.

It disables DVD, Blu-ray, CD audio, Lua, JavaScript, VapourSynth/SVP, Vulkan,
archive support, Rubber Band, capture devices, and the standalone mpv/FFmpeg
programs. FFmpeg external codec libraries are limited to dav1d; encoders are
limited to AC-3 (audio conversion/passthrough) and PNG/MJPEG (screenshots).
FFmpeg's GPL-only features are disabled. mpv itself uses its GPL-2.0-or-later
build because upstream gates X11 support on GPL; this differs from the Windows
LGPL build. Linux has no SVP profile or Windows graphics/TLS dependencies.
libplacebo 7.351.0 supplies required helper
functions without its separate GPU backends; MediaFlick renders through mpv's
traditional OpenGL path, not `gpu-next`.

## Build

Release builds use Ubuntu 24.04 x86-64, with baseline x86-64 instructions and
runtime CPU detection. Native aarch64 builds are also accepted by the script;
there is no cross-compilation or Linux arm64 release job. Building on a newer
distribution raises the resulting runtime's host-library requirements.

Install the build dependencies, and enable matching source repositories so the
build can collect the exact sources of redistributed distribution libraries:

```sh
sudo sed -i 's/^Types: deb$/Types: deb deb-src/' /etc/apt/sources.list.d/ubuntu.sources
sudo apt-get update
sudo apt-get install -y build-essential git meson ninja-build nasm pkg-config \
  python3 python3-jinja2 patchelf zstd libass-dev liblcms2-dev libdav1d-dev \
  libgnutls28-dev zlib1g-dev libasound2-dev libpulse-dev libegl1-mesa-dev \
  libgl-dev libva-dev libdrm-dev libx11-dev libxext-dev libxpresent-dev libxrandr-dev libxss-dev
just libmpv
```

On Debian or installations using `sources.list`, enable matching `deb-src`
entries instead. The build fails if a bundled library cannot be attributed to
an installed package, its notice is missing, or its exact source version is no
longer available in the configured repositories. Update the development
packages and their runtime dependencies together if source versions expire.

The reusable source/build cache defaults to
`$XDG_CACHE_HOME/mediaflick/libmpv-linux-<arch>` or
`~/.cache/mediaflick/libmpv-linux-<arch>`. Set `MEDIAFLICK_LIBMPV_WORK_DIR` to
override it and `MEDIAFLICK_LIBMPV_JOBS` to limit build parallelism. Modified or
unexpected upstream revisions are rejected; use a fresh cache when changing
pins. The script recreates its install prefix and output `lib/` and `licenses/`
directories, and never installs into `/usr`.

Output defaults to `build/libmpv-linux-<arch>`; an optional first argument to
`build.sh` overrides it. The payload contains private shared libraries,
checksums, build configurations, upstream revisions, distribution package
versions, license notices, and `mediaflick-libmpv-linux-<arch>-sources.tar.zst`.
The archive includes pinned upstream sources, exact Debian/Ubuntu source
packages and patches, and the build scripts/configuration. Publish it beside
every AppImage that includes the runtime.

## AppImage integration and validation

`just linux-appimage` builds this runtime, the release app, and the AppImage.
For an already built release and runtime, run
`bash distribution/linux/build-appimage.sh`; set `MEDIAFLICK_LIBMPV_DIR` if the
payload is outside its default directory. Packaging requires the runtime and
source archive, verifies checksums, and smoke-tests the relocated library.

The payload lives in `usr/bin/libmpv`, which the app's existing loader checks
before the system library. Each private library has `$ORIGIN` RUNPATH so its
dependencies resolve after relocation without putting codec libraries on the
global CEF search path. Notices and manifests live under
`usr/share/doc/mediaflick-desktop/libmpv`. The source archive is copied beside
the AppImage in `dist/linux` and uploaded by the existing draft-release job.

libc, C/C++ runtime support, X11, EGL/OpenGL, VA-API and GPU drivers, and ALSA/
PulseAudio interfaces remain host dependencies, listed in `HOST-LIBRARIES.txt`.
The bundler stops at those interfaces instead of copying vendor drivers or
private audio plugins. Fontconfig uses the host's font configuration/fonts;
GnuTLS uses the host's certificate trust store. A working X11/XWayland desktop
and EGL/OpenGL 3.3 driver are still required. Select **Built-in player**, save,
and restart; Linux's existing backend preference/default is unchanged.

```sh
python3 -m unittest discover -s distribution/libmpv/linux -p 'test_*.py'
python3 distribution/libmpv/linux/smoke-test.py build/libmpv-linux-x86_64/lib/libmpv.so.2
```

The smoke test checks client API major 2, render API symbols, the compiled
feature profile and common decoders, then loads and advances a generated WAV.
It does not establish real GPU rendering, hardware decoding, audio-device
output, or Jellyfin playback. Use the opt-in native runtime test described in
[`BUILDING.md`](../../../BUILDING.md) and a real desktop session for those.
