### Fixed

- Transcoded playback no longer hands the Jellyfin access token to mpv in the stream URL; the token is sent only in request headers, as it already was for direct play.
