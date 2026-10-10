### Added

- `just verify <drive>` drives the staged app for verification on an inactive private desktop (Windows) or a private Xvfb display (Linux) with a disposable profile, and keeps screenshots, accessibility snapshots, logs and the written settings under `build/verify/<run-id>`. The user's own profile is never written and the user's running MediaFlick is never touched. `just verify-doctor` checks readiness and `just verify-cleanup` removes what interrupted runs left behind.
