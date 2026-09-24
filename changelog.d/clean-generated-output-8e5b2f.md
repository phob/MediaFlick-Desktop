### Changed

- `just clean` now returns the checkout to a fresh state by also removing packaged `dist` and `release` output, installed UI dependencies and bundle, Companion build and restore output, and test and Python caches. Local configuration such as `.env` and the reusable CEF and libmpv caches outside the checkout are kept.
