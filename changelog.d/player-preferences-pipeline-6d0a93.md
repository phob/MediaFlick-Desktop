### Fixed

- The mark-watched-and-play-next key is saved with the other Player settings, so a failed save can no longer leave the key and the rest of the settings out of step. The key you set earlier is kept.
- Subtitle styling changes for the built-in player take effect from the next file that plays, without the player reading settings from disk mid-playback.
- The settings folder is no longer created under the current working directory when the configured system path is relative.
