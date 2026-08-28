# libmpv integration

## Decision

MediaFlick uses dynamically loaded libmpv as the default player on fresh
Windows installations. External mpv and MPC-HC remain supported. Linux and
macOS keep external mpv as their default until equivalent native library
bundles exist.

The first implementation deliberately lets libmpv create a separate native
video window. Rendering into the CEF surface would require mpv's render API,
graphics-context sharing, resize and DPI coordination, input routing, and
platform-specific window lifecycle work. That is a separate project, not a
requirement for zero-setup playback.

## Runtime shape

The existing `MpvController` remains the only owner of mpv playback state. Its
runtime is either an external child process or an in-process libmpv handle.
Both start the same unique `input-ipc-server`, so commands, observed events,
segment skipping, Jellyfin reporting, next-episode handoff, and recovery keep
using the mature JSON IPC path.

The library adapter loads only the small client API surface needed to create,
initialize, poll, and destroy a handle. Dynamic loading is intentional: a
missing or incompatible DLL becomes a normal backend startup error instead of
preventing MediaFlick from launching when another backend is selected. The
primary libmpv event queue is drained during supervision because an undrained
queue can fill even though playback events are consumed through JSON IPC.

Built-in mode starts with `config=no` and `load-scripts=no`. This makes the
bundled runtime deterministic and keeps it independent of a user's external
mpv files. Users who need scripts, shaders, SVP, or their existing `mpv.conf`
should select External mpv.

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
`packaging/libmpv/windows/README.md`. `MEDIAFLICK_DESKTOP_LIBMPV_PATH`
overrides discovery for development and smoke tests.

The app uses a baseline x86-64, shared libmpv DLL with its dependencies linked
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
