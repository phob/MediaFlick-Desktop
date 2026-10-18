### Changed

- Internal: Clippy now rejects plain `std::fs::write` calls and calls to the two startup-only `settings.json` entry points elsewhere, so durable files keep going through the atomic save path.

### Fixed

- A poster download cut short, or two requests for the same poster at once, can no longer leave a broken image in the poster cache.
