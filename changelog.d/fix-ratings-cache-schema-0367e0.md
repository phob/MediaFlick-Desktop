### Fixed

- Ratings cached under a different cache format are now fetched again from the Companion instead of being relabelled as current and shown as they were. If that fetch fails, those ratings are left out rather than shown, and the cached row is left untouched.
