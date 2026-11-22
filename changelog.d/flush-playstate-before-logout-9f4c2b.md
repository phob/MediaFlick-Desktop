### Fixed

- Signing out while watching a video or listening to audio now stops playback and sends the final resume position and watched status to Jellyfin before ending the server session, with a timeout so an unresponsive server cannot block sign-out.
