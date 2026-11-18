# Released `library.db` schemas

`released/` holds what a released MediaFlick wrote to `library.db`, so the
tests can prove that every later build opens it without losing a row. Nothing
in `released/` is ever edited: `src/library/db/tests.rs` holds each schema
file's SHA-256 (`RELEASED_SCHEMAS`) and fails when one changes.

- `schema-<N>.sql`: the executable schema the release created, at
  `PRAGMA user_version` N. SQLite's own objects and the FTS shadow tables are
  left out; SQLite creates them when the statements run.
- `rows-23.sql`: rows from the `library.db` the released 0.2.0 AppImage wrote
  after a first sync of a real Jellyfin 12.2 library with the MediaFlick
  Companion, a few per table with their relations kept, sanitized. A few rows
  are synthetic and say so: a sign-in token, the cache-owner keys, and the
  staging tables, which were empty on that profile.

`../preferences/released/0.2.0/` holds the JSON files the same 0.2.0 run wrote,
sanitized the same way; `src/preferences/released_formats.rs` loads them.

`scripts/fixtures/freeze_release.py` writes all of these. It replaces every id,
title, file name and account name with a consistent fake, so the raw profile,
which holds a real server's library, never enters the repository.

## Freeze a new schema

A schema counts as released with its first prerelease. When the release PR
publishes a `SCHEMA_VERSION` (`src/library/db/migrate.rs`) that has no
`schema-<N>.sql` yet, `scripts/prepare-release.py` refuses it. To freeze it:

1. Start the release build on a fresh profile, or run it with
   `--library-stats` with `XDG_DATA_HOME` pointing at an empty directory, so it
   creates `library.db`.
2. Run `python3 -I scripts/fixtures/freeze_release.py schema <that library.db>`.
3. Add the file's SHA-256 to `RELEASED_SCHEMAS` in `src/library/db/tests.rs`.

## Make a 0.2.0 profile for `just upgrade-e2e-0-2-0`

The upgrade proof needs a profile the released 0.2.0 AppImage left, signed in
to the session `just verify-login SERVER` cached, on a server with the
MediaFlick Companion. Make one with the released AppImage and
`scripts/e2e/profile-0-2-0.mjs`, which sets a choice on each settings shelf,
adds a My Collection with a custom poster and saves a subtitle choice:

```sh
mkdir -p build/e2e/mediaflick-0.2.0
gh release download v0.2.0 -D build/e2e/mediaflick-0.2.0 -p 'MediaFlickDesktop-v0.2.0-linux-x86_64.AppImage'
sha256sum build/e2e/mediaflick-0.2.0/*.AppImage   # f10515096ab2e5fe34b8c4f8140a96b147c5fef3ae52cc85d8ffd2bcaac71772
chmod +x build/e2e/mediaflick-0.2.0/*.AppImage
MEDIAFLICK_DESKTOP_EXE=$PWD/build/e2e/mediaflick-0.2.0/MediaFlickDesktop-v0.2.0-linux-x86_64.AppImage \
MEDIAFLICK_VERIFY_HARD_TIMEOUT=1800 \
  bash .claude/skills/verify-mediaflick/scripts/isolate-linux.sh node .claude/skills/verify-mediaflick/scripts/session.mjs \
  scripts/e2e/profile-0-2-0.mjs --account https://jellyfin.example.com --keep-profile --timeout 1500
```

`steps.log` ends with `kept profile <dir>`. Pass that directory as
`just upgrade-e2e-0-2-0 --account SERVER --profile <dir>`; the proof copies it
and never writes to it. It still holds the session's token in `library.db`, so
keep it private, and remove it with `just verify-cleanup` when you no longer
need it.
