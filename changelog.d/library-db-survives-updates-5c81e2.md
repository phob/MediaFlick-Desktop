### Changed

- A library database written by a newer MediaFlick is left as it was instead of being opened by an older version: install that newer version again to keep using it. Before, an older version ran on it anyway.
- When MediaFlick cannot open its files at startup, it tries once instead of again on every request until you restart it.

### Fixed

- Updating MediaFlick keeps your library database. From the database MediaFlick 0.2.0 created on, a later version adds what it needs to it and never deletes and rebuilds it, so you stay signed in on the same Jellyfin device and keep your library, franchises, collections and ratings without waiting for a new sync. Going back to 0.2.0 after an update keeps working.
- Custom collection posters are no longer deleted at startup when `collections.json` is missing.
