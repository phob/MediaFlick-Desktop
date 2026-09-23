# libmpv integration

## Decision

MediaFlick uses dynamically loaded libmpv as the default player on fresh
Windows installations. Linux supports the same React playback overlay using
a bundled AppImage runtime or system libmpv on X11 or XWayland. Linux and macOS retain external mpv
as their default; existing explicit backend choices and mpv paths are preserved.
External mpv remains supported. MPC-HC support was removed; a saved MPC-HC
selection loads as the platform's standard backend.

Windows uses the integrated player in the normal build and the normal `just
run` workflow whenever Built-in player was selected when MediaFlick started.
External mpv continues through the CEF Views shell.

## Integrated Windows rendering

The Windows layout follows Jellium Desktop's ownership model. libmpv creates
and owns the real top-level video window and publishes its HWND through mpv's
read-only `window-id` property. MediaFlick does not pass `wid` to mpv.

CEF runs windowless with shared textures enabled. Its accelerated paint
callback supplies D3D textures, which MediaFlick copies into premultiplied DXGI
swap chains attached to DirectComposition visuals on the mpv window. The main
view and CEF popup each have a visual. If CEF supplies a software paint frame,
the same D3D/DirectComposition path uploads it as a fallback; there is no
layered software window.

A transparent child HWND covers mpv's client area and forwards mouse and
keyboard events to CEF. MediaFlick tracks the mpv client size, DPI, and screen
position and updates both the child window and windowless browser. During
playback the React document exposes the video below it while retaining the
MediaFlick mark and player controls.

The shell warms libmpv in windowed mode with mpv's native border enabled. The
saved fullscreen preference is applied when playback starts, not while the
idle window is acting as the application shell. CEF cursor changes are mapped
onto the child HWND, and mpv's own cursor input and autohide are disabled so
the two input owners cannot fight. Alt+F4 and the native title-bar close command
are routed through CEF's browser shutdown lifecycle. MediaFlick also replaces
mpv's inherited big and small window icons with the icon embedded in the
application executable.

While a video is active, the input child retains the familiar built-in-player
bindings: right-click toggles pause, Q stops the current video without quitting
MediaFlick, and V cycles mpv's subtitle visibility. Those events go through the
playback coordinator instead of competing with mpv's native input thread. When
playback is idle, the same keys and right-click continue to reach React.

The backend choice is startup-bound because it selects the native window and
CEF composition model. Saving a different backend records the preference and
asks the user to restart MediaFlick; the running player is not rebuilt into a
different window model in place.

## Integrated Linux rendering

Select **Built-in player** in Settings → Player, save, and restart. AppImages
include libmpv; unpackaged builds need mpv 0.41 or newer with client API major 2,
gpu-next, and the X11 Vulkan/EGL backends. An X11 desktop
and working Vulkan or EGL/OpenGL driver are required; Wayland sessions use XWayland
and must provide `DISPLAY`. Native Wayland composition is not implemented.

MediaFlick owns the top-level window and video container. libmpv uses
`vo=gpu-next` and creates a video child inside that container through `wid`.
The ordered contexts `x11vk,x11egl` prefer Vulkan and fall back to OpenGL if
Vulkan initialization fails. Software GPU drivers remain usable in virtual
desktops. Hardware decoding stays on `auto-safe`, with interoperation owned by
mpv/libplacebo. mpv owns rendering, synchronization, and presentation; the app
does not create an `mpv_render_context` or a second EGL compositor.

CEF runs windowless on X11 with GPU compositing disabled because this adapter
consumes software frames. The shell sends one owned premultiplied BGRA bitmap
at a time to the playback controller. A synchronous, named `overlay-add`
command copies those pixels into mpv before returning the buffer to CEF.
gpu-next blends the bitmap over video, including while idle or paused. The
bitmap's display dimensions match the video container, keeping browser raster
pixels separate from X11 coordinates. An input-only sibling above the container
forwards events without allocating another composited drawable. Focus transfers
through mpv's video child remain inside the browser's focus scope.

Popup pixels are composed into the browser frame before submission. The owned
buffer returns after mpv copies it; old retained browser frames are released
on resize. Linux waits for a transparent playback frame and the video restart
following the delayed resume seek before applying automatic fullscreen. This
avoids overlapping catalog, decoder, and fullscreen buffer replacements. The
shell combines intermediate window-manager sizes and resizes the video
container after 250 ms without another size change; manual fullscreen requests
change the outer window immediately.

The overlay reads X11's `Xft.dpi` and keeps browser layout coordinates separate
from drawable pixels, including mouse input and popup placement. It updates CEF
when the desktop DPI changes. Under GNOME fractional scaling, XWayland can expose
a drawable larger than the actual monitor: a 4K output at 125% can report
6144 × 3456 X11 pixels with a 2× UI scale. The adapter queries Wayland output
metadata to cap browser rasterization at the selected monitor's physical density:
this example uses a 3072 × 1728 logical view and a 3840 × 2160 browser bitmap.
gpu-next scales that bitmap into X11 coordinates. Layout, pointer input,
and popup raster coordinates remain separate. If Wayland output metadata is
unavailable, the adapter uses X11 DPI for rasterization too. gpu-next's video
swapchain follows the X11 drawable size; unlike the previous OpenGL render-API
path, video rendering is not capped to physical monitor density. Fractional
scaling can therefore increase video rendering cost. The oversized X11 dimensions
must not be treated as the physical monitor or RDP output resolution.

The shell forwards pointer, keyboard, focus, and popup events to the existing
React interface, and native close requests use CEF's shutdown lifecycle.
Window bounds, maximized state, application identity, and the startup-ready
reveal remain app-owned. `--hidden` suppresses the initial reveal.

## Runtime shape

The existing `MpvController` remains the only owner of mpv playback state. Its
runtime is either an external child process or an in-process libmpv handle.
Both start the same unique `input-ipc-server`, so commands, observed events,
segment skipping, Jellyfin reporting, next-episode handoff, and recovery keep
using the mature JSON IPC path.

Both runtimes also share one resume path on every platform. They keep the
persistent IPC writer and apply resume as a delayed seek after `file-loaded`,
holding the reported Jellyfin position until that seek settles. Neither uses
load-time `start` or sends a timing-related startup `pause=false` command.

The library adapter loads only the small client API surface needed to create,
initialize, poll, and destroy a handle. Dynamic loading is intentional: a
missing or incompatible DLL becomes a normal backend startup error instead of
preventing MediaFlick from launching when another backend is selected. The
primary libmpv event queue is drained during supervision because an undrained
queue can fill even though playback events are consumed through JSON IPC.

The standard built-in profile starts with `config=no` and `load-scripts=no`.
This keeps the bundled runtime independent of a user's external mpv files. On
Windows, MediaFlick checks the per-user and machine-wide Windows uninstall
registry, including both registry views, for an SVP 4 installation when it
starts. It reads `InstallLocation`, falls back to the directory containing the
registered uninstaller, and then checks the standard Program Files directories.
It validates that the resulting `mpv64` directory contains `VSScript.dll`.
When valid, MediaFlick enables Lua scripts, uses `hwdec=auto-copy`, enables every
hardware-decoded codec, disables framedrop during precise seeks, and exposes
`\\.\pipe\mpvpipe`. MediaFlick also registers SVP's `mpv64` directory for DLL
loading and prepends it to `PYTHONPATH`. The fixed pipe is available if the user
starts SVP after MediaFlick. External mpv remains the backend for arbitrary
scripts, shaders, and complete `mpv.conf` compatibility.

## Settings and migration

An explicit `player_backend` always wins. For settings written before libmpv
existed, a saved `mpv_path` selects external mpv. A pathless legacy or fresh
Windows configuration selects libmpv. Passing `--mpv-path` also selects the
external backend. This prevents an upgrade from silently bypassing an existing
power-user setup.

## Packaging

Windows releases put `libmpv-2.dll` beside the application. Developer builds
also recognize `build/libmpv-windows-x64/libmpv-2.dll`, the build script's
default output. Details and license requirements are in
`distribution/libmpv/windows/README.md`. `MEDIAFLICK_DESKTOP_LIBMPV_PATH`
overrides discovery for development and smoke tests.

Linux checks beside the executable first, then probes `libmpv.so.2` through the
system dynamic loader, respecting its cache, multiarch directories, and
`LD_LIBRARY_PATH`. A missing or incompatible system runtime leaves Built-in
player unavailable in Settings. Linux AppImages bundle their own runtime and
codec/subtitle dependencies in `usr/bin/libmpv`, with origin-relative library
search paths, notices, checksums, and a separate corresponding-source archive.
The Linux build omits DVD, Lua, JavaScript, and VapourSynth while keeping
VA-API, NVDEC/CUDA interoperability, OpenGL and Vulkan (including libplacebo's
`gpu-next` backends), ALSA/PulseAudio, GnuTLS HTTPS, libass, and color management.
The app uses the gpu-next composition path described above, preferring Vulkan.
Host graphics/audio interfaces and drivers remain required. See
[`distribution/libmpv/linux/README.md`](../distribution/libmpv/linux/README.md).

The Windows app uses a baseline x86-64, shared libmpv DLL with its dependencies linked
into that DLL. Built-in mode requests mpv's safe automatic hardware decoding.
The tailored build keeps D3D11/D3D11VA, OpenGL, libass, color management,
FFmpeg network protocols, Schannel TLS, and common codecs while
dropping the command-line player, scripting engines, Vulkan, optical-disc and
archive integrations, external encoding libraries, and GPL-only FFmpeg features.

## Operational risks

In-process playback reduces setup and packaging friction, but a native crash or
deadlock in libmpv now affects the app process. External mpv remains the
isolation and customization fallback. The runtime is owned by the controller
thread and no libmpv handle crosses threads. Shutdown can still block inside
`mpv_terminate_destroy`; if that becomes observable in production, the next
step is a small bundled helper process rather than unsafe forced thread
termination.
