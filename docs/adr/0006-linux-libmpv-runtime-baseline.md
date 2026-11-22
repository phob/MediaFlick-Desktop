# 0006. Linux AppImage libmpv keeps GPU paths and builds on Ubuntu 24.04

Status: Accepted (2026-11-22). Decided in `a532c976` and `732c3239`
(2026-09-13) and #382 (runner pin, 2026-11-21).

## Context

Linux AppImages bundle their own libmpv
([0001](0001-built-in-libmpv-player.md)). The Windows build drops Vulkan and
renders through D3D11. On Linux, playback targets Vulkan rendering and
NVDEC hardware decoding ([0002](0002-native-video-composition.md)), and an
AppImage runs on the host's C library and graphics drivers.

## Decision

The Linux runtime drops only what the built-in player never uses: DVD and other
disc support, Lua, JavaScript and VapourSynth. It keeps Vulkan and OpenGL
rendering, VA-API, and NVDEC with CUDA interop. It bundles its private codec
and subtitle libraries but never GPU drivers, the Vulkan loader, or NVIDIA's
`libcuda` and `libnvcuvid`, which load from the host driver only when used.
mpv is built with `gpl=true` because upstream gates X11 support on it; FFmpeg
stays without GPL-only and nonfree features.

The AppImage and its libmpv build on the `ubuntu-24.04` runner, which Renovate
does not move. The exact build profile is in
[`distribution/libmpv/linux/README.md`](../../distribution/libmpv/linux/README.md).

## Rejected alternatives

- Matching the Windows profile by dropping Vulkan: the operator rejected it,
  because slimming must not remove normal GPU playback.
- Bundling GPU drivers or NVIDIA's libraries: they must match the host kernel
  driver, and an AMD or Intel host must load libmpv without them.
- Building on the newest runner: the glibc and other host-library versions it
  links against become the minimum the AppImage runs on.

## Consequences

- Moving the release runner is a deliberate support decision: it raises the
  oldest distribution the AppImage runs on, invalidates the libmpv build cache
  keyed on the runner, and changes the exact Ubuntu source packages collected
  for the corresponding-source archive. The libplacebo patch also guards
  Vulkan 1.4 code against Ubuntu 24.04's older Vulkan headers.
- A local build on a newer distribution is not evidence that the release
  artifact runs on the baseline.
- Linux and Windows libmpv builds differ in licence (GPL and LGPL) and in
  features, so a capability on one platform says nothing about the other.
