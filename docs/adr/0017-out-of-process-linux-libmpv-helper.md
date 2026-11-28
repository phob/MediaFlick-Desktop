# 0017. Out-of-process Linux built-in libmpv helper and bounded native player shutdown

Status: Accepted (2026-11-28). Supersedes [0001](0001-built-in-libmpv-player.md).

## Context

Running `libmpv` inside the main UI process allowed any fatal decoder or driver fault (`SIGSEGV`/`SIGABRT`) inside FFmpeg/libmpv/GPU drivers to crash the entire browser shell, and allowed a stalled `mpv_terminate_destroy` call (up to 60 s controller shutdown wait) to block application exit.

At the same time, Windows and Linux impose fundamentally different windowing constraints:

- **Linux (X11/XWayland)** separates the outer window from `libmpv` via `--wid=<XID>`: MediaFlick's main process owns `HostWindow` and binds `libmpv` into its `content` child window.
- **Windows (DirectComposition / DXGI)** requires `IDCompositionDevice::CreateTargetForHwnd` and flip-model swap chains (`DXGI_SWAP_EFFECT_FLIP_DISCARD`) to be owned by the same process as the target `HWND` (`ACCESS_DENIED` / `DXGI_ERROR_INVALID_CALL` across processes), and cross-process `SetParent` couples thread input queues and causes GUI deadlocks.

## Decision

Built-in playback retains the startup runtime selection, dynamic `libloading`, isolated `config=no`/`load-scripts=no` defaults (except SVP 4 on Windows), and external `mpv` fallback from [ADR 0001](0001-built-in-libmpv-player.md), but splits host isolation per platform:

- **Linux (`--player-helper`).** The main process creates and owns `HostWindow` (`window` + `content` subwindow) and spawns a dedicated child process (`mediaflick-desktop --player-helper`, with `LD_PRELOAD` stripped of `libcef.so`) that loads `libmpv` and binds `wid=<content_xid>` out-of-process. Parent and helper exchange overlay frames over a private `0o600` tmpfs file and a `250 ms`-timed `UnixStream` control channel. Death safety relies on `prctl(PR_SET_PDEATHSIG, SIGTERM)`, parent PID verification, and a `stdin`-EOF watchdog with a `1.2 s` hard-exit failsafe. If the helper crashes mid-session, `HostWindow` and the CEF overlay survive and MediaFlick respawns a fresh helper under a 5-restarts-per-30-seconds circuit breaker. On shutdown or hang (`SIGSTOP`/deadlocked `libmpv`), the parent bounds graceful quit to `750 ms` before sending `SIGKILL`.
- **Windows.** `libmpv` remains in-process so DirectComposition shared-texture composition and flip-model HDR/MPO swap chains continue to own the top-level `HWND` directly. `configure_libmpv_options` upgrades Windows to `vo=gpu-next` (`d3d11-flip=yes`, `target-colorspace-hint=auto`), `compositor.rs` fixes swap-chain flip mode, non-blocking `Present(0, ...)`, NT shared handle opening (`OpenSharedResource1`), keyed-mutex synchronization, `ResizeBuffers`, DXGI HDR color spaces, and device-lost recovery, and `LibMpvRuntime::terminate` bounds `mpv_terminate_destroy` to `1.5 s` before detaching and intentionally keeping the loaded DLL mapped so shutdown never deadlocks or unmaps active driver threads.

## Rejected alternatives

- Cross-process `SetParent` / child `HWND` hosting for Windows `libmpv`: breaks DirectComposition `CreateTargetForHwnd` process affinity and introduces cross-process Win32 input-queue deadlocks.
- Shared `wayland`/X11 in-process `libmpv` on Linux: leaves the browser shell vulnerable to native FFmpeg/GPU crashes and `mpv_terminate_destroy` exit hangs.

## Consequences

- On Linux, a fatal `libmpv` crash stops only the active playback item cleanly, keeps the main window open, and recovers playback on the next command without restarting CEF.
- On Linux, overlay frames pass through a `0o600` shared file and `250 ms` socket handshake per frame until replaced by zero-copy DMA-BUF Wayland composition (milestone 5).
- Windows built-in crash isolation still relies on external `mpv` when fault isolation from GPU/decoder drivers is required, though shutdown hangs are bounded under `2 s`.
