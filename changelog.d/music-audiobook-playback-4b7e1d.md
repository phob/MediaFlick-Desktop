### Added

- Music albums, tracks and audiobooks play in the app through the Built-in player and External mpv. An album page lists its tracks by disc with Play, Shuffle and a Play button per track; an audiobook page offers Continue at the saved position and From start.
- While audio plays you keep browsing: a now-playing bar shows the cover, title, artist, progress, pause, previous, next and stop. Tracks advance on their own, Previous restarts a track after its first 3 seconds, and the media keys work anywhere in the app; Space and the arrow keys work when no control has focus. Jellyfin sees the whole queue and the current entry in every playback report.

### Changed

- Audio never opens a player window, never takes the default fullscreen and never raises External mpv, and a right click pauses only video. Signing out or switching accounts stops an audio queue.

### Fixed

- Pausing while a title loads no longer leaves the player bar showing it as playing.
