### Fixed

- Builds made with Rust 1.100 package the Chromium runtime again: the Linux AppImage, the Windows installer and zip, and `just build` were missing `libcef` and its resources, so packaging failed and a locally built app could not start.
