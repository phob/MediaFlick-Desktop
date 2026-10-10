### Fixed

- Building the app no longer rewrites `ui/pnpm-lock.yaml`. The UI now uses pnpm 12.11.2, which fixes the 12.4.x bug that dropped the `@pnpm/exe` entry on every script run.
