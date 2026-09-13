# Linux libmpv handoff

## User intent and scope

Build a dedicated, slimmer Linux libmpv and bundle it in the AppImage. Disable
DVD, Lua, and VapourSynth. Preserve OpenGL, Vulkan, VA-API, and NVIDIA NVDEC/CUDA
interoperability. The user explicitly rejected removing Vulkan to match the
Windows profile. Removing scripting/disc extras must not remove normal GPU
playback capabilities.

The packaging work is merged. The follow-up also switches MediaFlick's embedded
Linux renderer to `gpu-next`, preferring Vulkan with an OpenGL fallback.

The initial Linux packaging implementation is commit `909a6bd` (`feat(linux):
bundle slim libmpv runtime in AppImages`). GPU build capabilities were restored
in `2003359` and merged through PR #149. Check `git status` and `git diff` for
the renderer follow-up's current commit status.

## Current implementation

- `distribution/libmpv/linux/build.sh`: native Linux build of mpv 0.41.0,
  FFmpeg 8.0.1, and libplacebo 7.351.0, pinned by commit. Uses a cache outside
  the checkout, with no system-wide installation.
- libplacebo retains OpenGL/Vulkan backends and glslang SPIR-V compilation.
  Its GLAD/Jinja/MarkupSafe/fast_float submodules are pinned by the parent tree.
  `libplacebo-python314.patch` fixes the pinned Vulkan XML generator on Python
  3.14; the build records its checksum and archives both patch and patched source.
- FFmpeg retains VA-API and NVDEC via ffnvcodec headers. NVENC and FFmpeg's
  Vulkan Video decoding are not enabled. Vulkan *rendering* is available in
  mpv/libplacebo independently of Vulkan Video decoding.
- `distribution/libmpv/linux/bundle.py`: copies the private dependency closure,
  sets `$ORIGIN` RUNPATH on every bundled library, and collects exact package
  sources/notices. Explicit build packages cover static shader compiler code
  and Vulkan/NVIDIA headers that do not appear in `ldd` output.
- The host supplies graphics/audio interfaces, Vulkan's loader, and actual GPU
  drivers. NVIDIA `libcuda.so.1`/`libnvcuvid.so.1` load on demand and are not
  bundled or required merely to load libmpv on an AMD/Intel machine.
- `distribution/linux/build-appimage.sh` installs libraries under
  `usr/bin/libmpv`, which the existing native discovery code checks before the
  system libmpv. It checks checksums and smoke-tests the relocated library.
- Notices/configuration/manifests go under
  `usr/share/doc/mediaflick-desktop/libmpv`. Corresponding sources are published
  beside the AppImage, not included inside its runtime filesystem.
- `.github/workflows/draft-release.yml` builds the runtime and publishes both
  the AppImage and Linux source archive, with Xvfb/Mesa smoke tests of both
  retained `gpu-next` backends on the staged payload. The release runner is Ubuntu 24.04
  x86-64. Local verification uses Ubuntu 26.04, so local artifacts should not
  be treated as proof of compatibility with the release baseline.
- Linux mpv uses `gpl=true` because upstream X11 support requires it. FFmpeg
  keeps GPL-only/nonfree features disabled. The existing Windows build is
  unchanged and retains its own Lua/VapourSynth/SVP choices.
- The earlier native startup fix in `src/players/mpv/runtime.rs` tolerates
  missing `load-scripts=no` and `osc=no` options when scripting was compiled
  out. Other errors remain strict, including SVP requesting scripts enabled.

## Embedded gpu-next rendering

`src/players/mpv/runtime.rs` sets `vo=gpu-next`, `gpu-api=vulkan,opengl`, and
`gpu-context=x11vk,x11egl`. The app still owns the X11 top-level window and
video container; `wid` lets mpv create its video surface inside the container.
The previous OpenGL render-context worker and its EGL/glow dependencies are
removed. VA-API/NVDEC selection remains mpv's responsibility through `auto-safe`.

`src/players/mpv/gpu_next.rs` sends owned, validated CEF BGRA frames through
synchronous, named `overlay-add` commands. mpv copies the pixels before the
buffer returns to CEF and composites them on the selected GPU, including while
idle or paused. Browser raster size and overlay display size remain separate.
The input-only window is a sibling above the video container, so newly created
mpv child windows cannot cover it. Focus handling includes mpv's child subtree.

The pinned render API itself still exposes only OpenGL and software backends:

- [mpv 0.41 render API](https://github.com/mpv-player/mpv/blob/v0.41.0/include/mpv/render.h)
- [mpv 0.41 render backend registration](https://github.com/mpv-player/mpv/blob/v0.41.0/video/out/vo_libmpv.c)

No OpenGL render context is driven alongside gpu-next. The standalone smoke
tests exercise the retained backends; the native composition regression in
`src/players/mpv/gpu_next/tests.rs` verifies the app's actual context and displayed
pixels, including scaling, transparency, paused video, and frame ownership.
The release workflow runs it on Vulkan and again with Vulkan drivers unavailable
to establish the OpenGL fallback, then runs the native focus regression.

CEF retains native-density rasterization, popup placement, and input scaling.
gpu-next's video swapchain follows the X11 drawable size. This can increase
video rendering cost under fractional XWayland scaling compared with the old
OpenGL render-API path's cap to physical monitor density. See
`docs/libmpv-integration.md` for the complete ownership and scaling model.

Before changing rendering, read `AGENTS.md`, `docs/libmpv-integration.md`,
`src/players/mpv/runtime.rs`, `gpu_next.rs`, `linux_window.rs`, and the Linux
implementation under `src/shell/cef/prototype_osr/`. Preserve the working
overlay, HiDPI scaling, popups, pointer/keyboard/focus handling, fullscreen,
window placement, and delayed startup seek. Preserve persistent IPC writes
and resume/playstate invariants; do not replace them with loadfile start
offsets, URL fragments, or an unconditional startup unpause.

## Verification from the merged packaging work

- ShellCheck and all four packaging regression tests passed, including host
  Vulkan/NVIDIA library exclusion and source collection for build dependencies.
- The native libmpv build and headless ABI/feature/decoder smoke test passed.
  H.264, HEVC, and AV1 each report compiled CUDA/NVDEC and VA-API configurations.
- Standalone `gpu-next` video playback passed with both `--gpu-api opengl`
  and `--gpu-api vulkan` under Xvfb using Mesa software rendering.
- The app's existing `configured_library_initializes_its_ipc_server` test
  passed with an H.264 clip through its embedded OpenGL renderer. This host
  has no NVIDIA driver: the `Cannot load libcuda.so.1` diagnostic confirms
  that software fallback worked, not that physical NVDEC decoding was tested.
- The source archive was checked for the Vulkan generator patch, its recorded
  checksum, code-generator sources/licenses, and shader/header package records.
- `just linux-appimage` passed, including a repeated build through the same
  patched source cache and the relocated payload's profile/decoder check.
  Both `gpu-next` rendering tests also passed against the staged AppDir.
  The rebuilt AppImage launched with `--version` and reported `0.1.6`.
- Release workflow YAML parsing and `git diff --check` passed. The workflow
  itself has not been executed on GitHub's Ubuntu 24.04 runner in this session.
- No Rust source changed in this follow-up. The original packaging/startup
  implementation had passed `just rust-quality` and `just test` (398 tests).

Useful local logs are `/tmp/mediaflick-preserve-gpu-build.log`,
`/tmp/mediaflick-gpu-next-opengl.log`, `/tmp/mediaflick-gpu-next-vulkan.log`,
`/tmp/mediaflick-preserve-gpu-native.log`, and
`/tmp/mediaflick-preserve-gpu-appimage.log`.

## Renderer follow-up verification

- `just rust-quality` and `just test` passed (397 passed, three opt-in tests).
- The native pixel regression passed with `current-gpu-context=x11vk`, and
  again with `x11egl` after disabling Vulkan driver discovery. Idle and paused
  composition, alpha blending, transparency, buffer reuse, and resizing passed.
- The native X11 focus regression passed with mpv's video child included.
- The native IPC/media test passed through the new renderer with H.264 video.
- The full CEF sign-in window rendered through `x11vk` under Xvfb; native
  mouse clicks and keyboard input reached the expected input field.
- The final AppImage and corresponding-source archive were rebuilt. Relocated
  profile tests and native composition passed on Vulkan and OpenGL. The actual
  AppImage reported `0.1.6`, loaded its bundled libmpv with `x11vk`, accepted
  native mouse/keyboard input, and exited successfully through `WM_DELETE_WINDOW`.
- ShellCheck, all four packaging regressions, workflow YAML parsing, archive
  checks, and `git diff --check` passed. The release workflow now includes the
  native composition/fallback and focus tests; it has not been run on GitHub.
- Physical AMD/NVIDIA decoding, a real Jellyfin playback session, window-manager
  fullscreen behavior, and the Ubuntu 24.04 GitHub release job still require
  verification in their respective environments.

New logs use `/tmp/mediaflick-handoff-*`. The pixel-test commands and the forced
OpenGL fallback invocation are documented in `BUILDING.md`.

## Build and validation commands

Install dependencies and enable matching `deb-src` repositories using
[`distribution/libmpv/linux/README.md`](../distribution/libmpv/linux/README.md).

```sh
just libmpv
just linux-appimage

shellcheck distribution/libmpv/linux/build.sh distribution/linux/build-appimage.sh
python3 -m unittest discover -s distribution/libmpv/linux -p 'test_*.py'
python3 distribution/libmpv/linux/smoke-test.py \
  build/libmpv-linux-x86_64/lib/libmpv.so.2 build/libmpv-linux-x86_64/LIBPLACEBO-CONFIG.h

xvfb-run -a python3 distribution/libmpv/linux/smoke-test.py \
  build/libmpv-linux-x86_64/lib/libmpv.so.2 build/libmpv-linux-x86_64/LIBPLACEBO-CONFIG.h --gpu-api opengl
xvfb-run -a python3 distribution/libmpv/linux/smoke-test.py \
  build/libmpv-linux-x86_64/lib/libmpv.so.2 build/libmpv-linux-x86_64/LIBPLACEBO-CONFIG.h --gpu-api vulkan

CEF_PATH="$(just --evaluate CEF_PATH)" \
CARGO_TARGET_DIR="$(just --evaluate CARGO_TARGET_DIR)" \
MEDIAFLICK_DESKTOP_LIBMPV_PATH="$PWD/build/libmpv-linux-x86_64/lib/libmpv.so.2" \
MEDIAFLICK_DESKTOP_LIBMPV_MEDIA_PATH=/path/to/test-video.mp4 \
xvfb-run -a cargo test configured_library_initializes_its_ipc_server -- --ignored

APPIMAGE_EXTRACT_AND_RUN=1 dist/linux/MediaFlickDesktop-v0.1.6-linux-x86_64.AppImage --version
git diff --check
```

Cache: `~/.cache/mediaflick/libmpv-linux-x86_64` (override with
`MEDIAFLICK_LIBMPV_WORK_DIR`). Runtime output: `build/libmpv-linux-x86_64`.
AppImage/source output: `dist/linux`. Do not edit generated build/dist files.
Do not edit a running Bash build script in place: Bash can read the modified
file at an old offset. Finish or stop the running build before changing it.

Physical AMD/NVIDIA hardware decoding and real Jellyfin playback need runtime
verification on suitable machines. Software-rendered Xvfb tests cannot
establish those results. Linux's existing backend default/preference is
unchanged; users select Built-in player, save, and restart to use this runtime.
