### Changed

- Helpers that existed in several copies now have one owner each: text trimming, the full-text search decision, Companion capability checks, the sort indexes, the ratings and technical-badge batch scheduler, local dates, media-URL tokens, civil dates and collection refresh failures. Behavior is unchanged. A media URL with an empty token before a filled one now hands the filled token to external mpv, as playback reporting already did.
- `/api/status` no longer repeats the Companion status; `/api/companion/info` is its only source.
- A new check, run in CI and `just check-pr`, fails a change that copies a block of code from one file into another (jscpd, counted against a ratchet that may only fall).
