### Fixed

- Built-in `libmpv` playback no longer tears down its host window and closes the app when an mpv IPC command times out or disconnects; instead, only the IPC worker is recycled against the live `libmpv` instance while external mpv keeps its full process restart behavior.
- On Linux, a `libmpv` UI overlay presentation timeout while the playback controller is busy (for example during a remote subtitle load or IPC reconnect) now skips the frame and catches up once the controller is free instead of closing the app.
