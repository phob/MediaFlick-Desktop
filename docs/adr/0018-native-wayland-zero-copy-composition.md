# 0018. Native Wayland subsurface video composition and DMA-BUF CEF UI on Linux

Status: Accepted (2026-11-28). Decided on branch `perf/deepen-player-spine`. Supersedes the Linux section of [0002](0002-native-video-composition.md); Windows DirectComposition and macOS external `mpv` remain unchanged.

## Context

Phase 1 measurements against bare upstream `mpv` (`waylandvk`) using the same GPU hardware (Intel HD 630 under GNOME Wayland) established the cost of [0002](0002-native-video-composition.md)'s Linux X11/XWayland composition path:

- Running CEF in windowless X11 software rendering (`--ozone-platform=x11 --disable-gpu-compositing`), copying the full framebuffer into a helper file on each paint, and uploading it through `mpv`'s `overlay-add` forced CPU pixel copies on every UI redraw and forced `mpv` through XWayland (`x11vk`) instead of native Wayland Vulkan (`waylandvk`).
- Under active React player UI overlays at 4K 10-bit HEVC HDR10 playback, bare `mpv --gpu-context=waylandvk` ran with direct Wayland presentation and PipeWire audio, while the XWayland `overlay-add` path paid extra swapchain indirection, CPU memory bandwidth, and full-frame PNG/BGRA upload synchronization timeouts on resize or heavy UI activity.

To reach bare-`mpv` parity under Wayland while retaining out-of-process `libmpv` crash and shutdown isolation ([0017](0017-out-of-process-linux-libmpv-helper.md)), both the UI layer and the isolated `libmpv` helper must present directly to the host Wayland compositor as stacked subsurfaces.

## Decision

When running on a Wayland session that advertises `wl_compositor`, `wl_subcompositor`, `xdg_wm_base`, `wp_viewporter`, and `zwp_linux_dmabuf_v1` (unless `MEDIAFLICK_FORCE_X11=1` is set):

- **Host top-level window and subsurfaces.** The main process runs a single-threaded, `poll(2)`-driven Wayland client and private `wayland-server` bridge (`wayland-host`) connected to the host Wayland compositor. It owns one `xdg_toplevel` root surface backed by an opaque single-pixel black background, a desynchronized `video` subsurface beneath with an empty input region, and a desynchronized transparent `ui` subsurface (plus optional `popup` subsurface) stacked above it.
- **Zero-copy helper video.** The isolated `--player-helper` receives `--backend wayland` and `WAYLAND_SOCKET=3` over an inherited `UnixStream` pair (with `WAYLAND_DISPLAY` and `DISPLAY` unset so it never talks directly to the desktop compositor), configuring `libmpv` with `gpu-context=waylandvk,wayland`, `ao=pipewire,pulse,alsa`, `target-colorspace-hint=auto`, `clipboard-backends=none`, and `clipboard-monitor=no` (so mpv's clipboard backend never consumes `WAYLAND_SOCKET=3` ahead of `gpu-next`). The private Wayland server proxies `zwp_linux_dmabuf_v1`, `wl_shm`, `wl_output`, `xdg_wm_base`, `wp_viewporter`, `wp_fractional_scale_v1`, `wp_presentation`, `wp_linux_drm_syncobj_v1`, `wp_fifo_v1`, `wp_commit_timing_v1`, `wp_color_representation_v1`, and `wp_color_manager_v1` directly to the host `video` subsurface and returns `wl_buffer.release` events without prematurely destroying Mesa swapchain buffers.
- **Zero-copy CEF UI.** CEF runs with `--ozone-platform=wayland --use-gl=angle --use-angle=gl-egl` and `shared_texture_enabled = 1`, forwarding `on_accelerated_paint` DMA-BUF file descriptors directly onto the `ui` subsurface (`on_paint` remains as an explicit `wl_shm` fallback) and waking the CEF UI thread only on compositor events rather than polling on a 16 ms timer.
- **X11 fallback.** Non-Wayland sessions, compositors lacking required globals, or `MEDIAFLICK_FORCE_X11=1` continue to use the X11/XWayland helper composition path from [0002](0002-native-video-composition.md).

## Rejected alternatives

- **Keeping XWayland + `overlay-add` on Wayland sessions:** Rejected because Chromium's software X11 OSR readback plus `overlay-add` CPU uploads cannot avoid full-frame copy overhead and prevents `mpv` from using `waylandvk`, `wp_color_manager_v1`, and native fractional scaling.
- **Moving `libmpv` back in-process on Wayland:** Rejected because in-process `libmpv` violates [0017](0017-out-of-process-linux-libmpv-helper.md) and exposes the entire UI process to `libmpv`/FFmpeg crashes and driver teardown deadlocks.
- **Reversing [0002](0002-native-video-composition.md) on Windows:** Rejected; Windows already keeps both `libmpv` and CEF on GPU via DirectComposition (`vo=gpu-next` + `d3d11`).

## Consequences

- `distribution/libmpv/linux` must bundle `libmpv.so.2` built with Wayland, EGL/Vulkan Wayland contexts, DMA-BUF, and PipeWire audio enabled.
- Wayland subsurface compositing isolates helper crashes (`SIGSEGV` detaches the `video` subsurface cleanly while the React UI survives) and keeps X11 fallback tests and sessions operational behind `MEDIAFLICK_FORCE_X11=1`.
- CEF cannot pass Vulkan flags under `--ozone-platform=wayland`; Chromium uses GL/EGL DMA-BUF pixmap export on the UI process while `--player-helper` uses Vulkan `waylandvk` independently.
- On Wayland, MediaFlick scopes `INTEL_DEBUG=noccs` and `AMD_DEBUG=nodcc` exclusively to Linux CEF `--type=zygote` and `--type=gpu-process` subprocesses at entry so Mesa allocates single-plane DMA-BUFs accepted by Chromium's `VideoFrame::WrapMappableSharedImage` (avoiding CEF 156 issue `#4354` multi-plane CCS/DCC rejection), while `--player-helper` keeps unmodified hardware compression for video decoding and presentation.
