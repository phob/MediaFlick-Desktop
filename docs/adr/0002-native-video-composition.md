# 0002. mpv presents video natively; CEF is composited over it per platform

Status: Accepted (2026-11-22). Decided in `09f3c18d` (Windows, 2026-08-29) and
`7621a928` (Linux gpu-next, 2026-09-13).

## Context

With the built-in player ([0001](0001-built-in-libmpv-player.md)), the React UI
draws the player controls over the video in one window. Video must keep mpv's
own renderer, hardware decoding and presentation; the browser layer must keep
its HiDPI layout, popups and input.

## Decision

mpv owns video rendering and presentation on both platforms. CEF runs
windowless, and its frames are composited above the video. Input reaches CEF
through a separate transparent or input-only window, not through mpv.

- **Windows.** libmpv creates and owns the top-level window; MediaFlick reads
  its handle from mpv's `window-id` and does not pass `wid`. CEF paints into
  shared D3D textures, which MediaFlick presents through DirectComposition
  visuals on that window, so the browser layer stays on the GPU. A transparent
  child window above mpv's client area forwards input to CEF.
- **Linux.** MediaFlick owns the X11 top-level window and a video container;
  libmpv renders into the container through `wid` with `vo=gpu-next`,
  preferring Vulkan (`x11vk`) and falling back to OpenGL (`x11egl`). CEF renders
  software frames, which go to mpv through synchronous `overlay-add` and are
  blended by gpu-next. An input-only sibling above the container forwards
  input.

## Rejected alternatives

- A layered software window for the browser on Windows: it moves every browser
  frame through the CPU. A software paint from CEF uses the same
  DirectComposition path as a fallback.
- Linux rendering through mpv's render API (`mpv_render_context`) with an
  app-owned EGL compositor, which an earlier version shipped: in mpv 0.41 the
  render API offers only OpenGL and software backends
  ([render.h](https://github.com/mpv-player/mpv/blob/v0.41.0/include/mpv/render.h)),
  so it rules out Vulkan, and the app would own a second compositor and its
  synchronization. With gpu-next through `wid`, mpv and libplacebo own
  rendering, presentation and hardware-decoding interop.
- Native Wayland composition: not implemented. The built-in player forces CEF
  onto X11, so Wayland sessions need XWayland and a `DISPLAY`.

## Consequences

- Linux CEF runs with GPU compositing disabled, because the overlay consumes
  CPU frames; each browser frame is copied into mpv.
- Under fractional scaling on XWayland the X11 drawable can be larger than the
  monitor (a 4K output at 125% reports 6144 × 3456). MediaFlick caps the
  browser raster at the monitor's physical density using Wayland output
  metadata, but gpu-next's video swapchain follows the X11 drawable, so video
  rendering costs more there than under the old render-API path. Never treat
  the oversized X11 size as the physical or RDP output resolution.
- macOS has no composition path for libmpv; its default is external mpv.
