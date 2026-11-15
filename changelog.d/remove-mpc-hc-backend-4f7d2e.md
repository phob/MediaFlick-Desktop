### Breaking Changes

- Removed the MPC-HC player backend. If you used MPC-HC, MediaFlick switches you to the built-in player when you upgrade and keeps your other settings.

### Changed

- The built-in player on Windows and macOS now resumes the same way as external mpv and Linux: it seeks after the file loads and holds the position reported to Jellyfin until the seek lands, instead of starting paused at the resume point.
