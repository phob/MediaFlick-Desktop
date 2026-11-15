### Added

- The MediaFlick Companion has a stable Jellyfin plugin repository at `https://flick.media/companion/manifest.json`, with an install guide at [flick.media/companion](https://flick.media/companion/): add the repository, install the Companion from the catalog, and restart Jellyfin. The Companion needs Jellyfin 12.0.0 or newer.

### Changed

- A Companion release can no longer become GitHub's latest release. If one does, a workflow moves Latest back to the newest desktop release, so desktop update checks and the website's download buttons keep working.

### Removed

- The opt-in Companion test repository on the `companion-test-repository` branch. It was never published, so its documented URL did not work.
