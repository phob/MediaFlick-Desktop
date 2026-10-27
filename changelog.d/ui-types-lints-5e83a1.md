### Changed

- The UI's checks now catch more mistakes before review: a query key must come from the shared key registry, media kind translations go through one module, raw form controls stay in the shared components, and knip reports unused code. Non-null assertions, empty fallbacks for query data and raw palette colors may only decrease, compared against the base branch. Behavior and appearance are unchanged.
