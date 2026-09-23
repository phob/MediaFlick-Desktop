### Fixed

- Playback no longer stops the app if its Jellyfin progress reporter cannot start. The failure is logged, and the title still plays.
- Local builds no longer rebuild the embedded app window after every `git add`. A new commit still updates the reported version.
