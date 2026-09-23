### Changed

- The built-in player on Windows and macOS now resumes the same way as external mpv and Linux: it seeks after the file loads and holds the position reported to Jellyfin until the seek lands, instead of starting paused at the resume point.

### Removed

- Removed the MPC-HC player backend. Installations that selected MPC-HC now start with the platform's standard player: the built-in player on Windows and external mpv elsewhere.
