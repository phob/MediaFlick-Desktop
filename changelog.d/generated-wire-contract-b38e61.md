### Changed

- The UI's types for the app status, settings, player state, player commands, item details and library filters are now generated from the Rust types the app sends (`ui/src/lib/api/wire.gen.ts`, rebuilt with `just wire-types`). A Rust test fails when the generated file is out of date, so the two sides can no longer drift apart.
- New API answers must use a named Rust type. A test counts the answers and request bodies that still use untyped JSON and lets that number only go down.

### Removed

- The unused `set-playback-rate` player command. Nothing in the UI sent it.
