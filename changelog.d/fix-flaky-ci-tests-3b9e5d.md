### Fixed

- Stabilized the Discovery back-navigation UI test, which timed out on slower CI runners, and the Companion request-retry client test, whose loopback server could reset connections on Windows before the client read its response.
