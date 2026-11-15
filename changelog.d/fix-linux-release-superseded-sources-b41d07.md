### Fixed

- The Linux release build no longer fails when Ubuntu has published an update to a bundled library that GitHub's runner image does not have yet. The job now brings the image's Ubuntu packages up to date first, so the AppImage ships the current libraries together with their sources.
