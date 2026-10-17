### Changed

- The UI's types for the app status, settings, player and item details are now generated from the Rust types the app sends, and a test fails when the two sides drift apart.

### Removed

- The unused `set-playback-rate` player command. Nothing in the UI sent it.
