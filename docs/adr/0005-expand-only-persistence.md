# 0005. Durable files evolve expand-only, because older releases keep opening them

Status: Accepted (2026-11-22). Decided in #357 (2026-11-18), when 0.2.0 was
Latest.

## Context

Before 0.2.0, a `library.db` schema change recreated the database and resynced
the catalog. 0.2.0 shipped schema 23 to real users, and that habit would now
destroy their Manual collections, caches and spent provider quota. Users also
move between versions in both directions, and released code is fixed:

- 0.2.0 opens a `library.db` from a newer schema, logs a warning and runs its
  own SQL against it.
- 0.2.0 upserts catalog rows with `ON CONFLICT DO UPDATE`, which leaves columns
  it does not know as they were.
- An older release's account switch clears only the tables it knows.
- 0.2.0's strict JSON types refuse to start on an unknown field or enum value;
  its lenient ones drop unknown fields on their next save.
- 0.2.0 deletes any file in `custom-art/` that `collections.json` does not
  name, once it is 7 days old.

## Decision

From schema 23, `library.db` changes only by appending a migration step that
adds tables, nullable or defaulted columns, indexes and triggers. A migration
that fails leaves the file as it was and keeps the services down; the database
is never recreated. User-intent JSON files gain fields with `#[serde(default)]`
and bump `version` only when an existing field changes shape or meaning. A
newer, unsupported file is refused without being rewritten. The rules are in
[AGENTS.md](../../AGENTS.md#persistence-and-compatibility) and enforced by the
checks listed there.

## Rejected alternatives

- Recreating or resyncing the database on a schema change: it destroys user
  data that no sync can restore.
- Renaming, dropping or tightening a column or table: an older release that
  opens the file still reads and writes it.
- An automatic fallback to a fresh database when a migration fails: a silent
  data loss in place of a visible error.
- Future-version wrappers or preserving unknown enum variants in JSON: no
  second format exists to justify them.

## Consequences

- Shipped migration steps are permanent. A wrong one is fixed by the next step,
  never edited.
- A column derived from others is kept current by a trigger, which an older
  release fires too. A new account-scoped table keys its rows by server and
  user.
- A table rebuild or a data drop or rewrite needs the operator's approval in
  its own PR that states the cost to users.
- There is no automatic backup, so a migration that commits wrong data is the
  remaining risk. Each step is proven against the frozen fixtures in
  `fixtures/library-db/released/` and the 0.2.0 upgrade drive.
- A new kind of upload gets its own directory, never `custom-art/`.
