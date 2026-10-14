### Removed

- Removed the pre-`settings.json` fallbacks. MediaFlick no longer reads device settings or shortcuts from the old `config.json` and `input.json` files; without `settings.json` it starts with defaults.
- Removed the MPC-HC mapping for saved settings. A `settings.json` that still selects MPC-HC is refused and left untouched. MediaFlick then starts with default settings and cannot save them until `player_backend` is set to `libmpv` or `mpv`, or removed, and the app is restarted.
- Removed repairs for caches written by earlier builds. A library cache that has items but no catalog-ready marker, and whose first sync has not finished, is no longer treated as ready, and Seerr images cached as error responses are refetched instead of deleted.
- Companion ratings now name each source once, as `sourceId`, instead of repeating it as `source` and `rawSource`.
