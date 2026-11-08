### Added

- Music albums, tracks and audiobooks play in the app through the Built-in player and External mpv. An album page lists its tracks by disc with Play, Shuffle and a Play button per track; an audiobook page offers Continue at the saved position and From start.
- While audio plays you keep browsing: a now-playing bar shows the cover, title, artist, progress, pause, previous, next and stop. Tracks advance on their own, Previous restarts a track after its first 3 seconds, and the media keys work anywhere in the app; Space and the arrow keys work when no control has focus. Jellyfin sees the whole queue and the current entry in every playback report.
- Jellyfin's Play On can send music and audiobooks to the app: a single track, a list of tracks or an album plays as a queue from the chosen item, and Previous and Next from the remote move through it. External mpv's own Next and Previous keys skip through the queue too.

### Changed

- Audio never opens a player window, never takes the default fullscreen and never raises External mpv, and a right click pauses only video. Signing out or switching accounts stops an audio queue.

### Fixed

- Pausing while a title loads no longer leaves the player bar showing it as playing.
- When the player cannot start a track, for example because the mpv executable in Settings is wrong, the album or audiobook now ends along with the error instead of the app waiting for a stop that never comes.
