#!/usr/bin/env python3
"""Freeze what a released MediaFlick Desktop wrote, as fixtures for the tests.

    python3 -I scripts/fixtures/freeze_release.py schema LIBRARY_DB [OUT]
    python3 -I scripts/fixtures/freeze_release.py 0.2.0 PROFILE [OUT]

OUT defaults to `fixtures/`.

`schema` writes `library-db/released/schema-<N>.sql`, N being LIBRARY_DB's
`PRAGMA user_version`: the executable schema that database holds, from
`sqlite_master` in creation order. SQLite's own objects (`sqlite_stat*`,
autoindexes) and the shadow tables of each virtual table are left out:
SQLite creates those itself when the statements run. Run it on the
library.db a fresh profile of the release build creates.

`0.2.0` takes PROFILE, a profile the released v0.2.0 AppImage left in the
verify layout (`config/mediaflick-desktop/`, `data/mediaflick-desktop/`; see
fixtures/library-db/README.md), and writes:

- `library-db/released/schema-23.sql`, as `schema` does;
- `library-db/released/rows-23.sql`: a few rows of every table, with their
  relations kept, and the rows that need a value the profile did not have
  (marked synthetic): a sign-in token, the cache-owner keys, one staged
  Letterboxd profile, one staged playback choice and one identity mapping;
- `preferences/released/0.2.0/*.json`: the JSON files 0.2.0 wrote, in its
  format (serde_json pretty, no final newline).

Sanitizing is deterministic: the same profile gives the same bytes. Every
server, user, device, item, library, profile and image id is replaced by a
fake id of the same shape in order of first appearance, and so are TMDB,
IMDb and TVDB ids. Titles, overviews, studios, tags, artwork paths, file
names, vote counts, the server URL, the user name and the Letterboxd account
become neutral values. The same value maps to the same fake everywhere, so
the relations between rows and files survive. The raw profile never goes
into the repository.
"""

from __future__ import annotations

import json
import re
import sqlite3
import sys
from pathlib import Path

HEX_ID = re.compile(r"^[0-9a-f]{32}$")
SYNTHETIC_TOKEN = "fixture-token-0f1e2d3c4b5a69788796a5b4c3d2e1f0"


class Fakes:

    def __init__(self) -> None:
        self.seen: dict[tuple[str, str], int] = {}

    def number(self, kind: str, value: str) -> int:
        key = (kind, value)
        if key not in self.seen:
            self.seen[key] = 1 + sum(1 for seen_kind, _ in self.seen if seen_kind == kind)
        return self.seen[key]

    def hex_id(self, value: str | None) -> str | None:
        if value is None:
            return None
        if not HEX_ID.match(value):
            raise SystemExit(f"expected a 32-digit hex id, got {value!r}")
        return f"f1c7{self.number('hex', value):028x}"

    def tmdb(self, value: int | str | None) -> int | str | None:
        if value is None:
            return None
        fake = 900000 + self.number("tmdb", str(value))
        return str(fake) if isinstance(value, str) else fake

    def collection(self, value: int) -> int:
        return 800000 + self.number("tmdb-collection", str(value))

    def imdb(self, value: str | None) -> str | None:
        return None if value is None else f"tt9{self.number('imdb', value):06d}"

    def tvdb(self, value: str | None) -> str | None:
        return None if value is None else str(70000 + self.number("tvdb", value))

    def label(self, kind: str, value: str | None) -> str | None:
        return None if value is None else f"{kind} {self.number(kind, value)}"

    def artwork(self, value: str | None) -> str | None:
        return None if value is None else f"/fixture-art-{self.number('art', value)}.jpg"


def literal(value: object) -> str:
    if value is None:
        return "NULL"
    if isinstance(value, bool):
        return "1" if value else "0"
    if isinstance(value, (int, float)):
        return repr(value)
    return "'" + str(value).replace("'", "''") + "'"


def schema(db: sqlite3.Connection) -> str:
    version = db.execute("PRAGMA user_version").fetchone()[0]
    virtual = [name for (name,) in db.execute("SELECT name FROM sqlite_master WHERE sql LIKE 'CREATE VIRTUAL TABLE%'")]
    shadows = {f"{name}_{suffix}" for name in virtual for suffix in ("data", "idx", "content", "docsize", "config")}
    statements = [
        f"{sql};"
        for name, sql in db.execute("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY rowid")
        if not name.startswith("sqlite_") and name not in shadows
    ]
    header = (
        f"-- The released library.db schema {version} (PRAGMA user_version), as the release build\n"
        "-- created it, in creation order. Frozen: never edit this file.\n"
        "-- Written by scripts/fixtures/freeze_release.py, which leaves out SQLite's own objects and the\n"
        "-- shadow tables of each virtual table; SQLite creates those when these statements run.\n"
    )
    return header + "\n".join(statements) + "\n"


def rows(db: sqlite3.Connection, fakes: Fakes) -> str:
    def select(sql: str, *args: object) -> list[dict[str, object]]:
        cursor = db.execute(sql, args)
        names = [column[0] for column in cursor.description]
        return [dict(zip(names, row)) for row in cursor.fetchall()]

    credentials = select("SELECT * FROM credentials WHERE id = 1")[0]
    server, user = credentials["server_id"], credentials["user_id"]

    playback_item = "00c25f6893ad550b7f44d74d74291d7e"
    members = select(
        """SELECT i.jellyfin_id FROM items i JOIN franchise_movie_membership m
           ON m.tmdb_id = CAST(i.tmdb_id AS INTEGER) AND m.collection_id IS NOT NULL
           WHERE i.kind = 'Movie' ORDER BY m.collection_id, i.jellyfin_id LIMIT 2"""
    )
    loner = select(
        """SELECT i.jellyfin_id FROM items i JOIN franchise_movie_membership m
           ON m.tmdb_id = CAST(i.tmdb_id AS INTEGER) AND m.collection_id IS NULL
           WHERE i.kind = 'Movie' ORDER BY i.jellyfin_id LIMIT 1"""
    )
    watched = select(
        """SELECT i.jellyfin_id FROM items i JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
           WHERE i.kind = 'Episode' AND (u.played = 1 OR u.playback_position_ticks > 0)
           ORDER BY i.jellyfin_id LIMIT 1"""
    ) or select("SELECT jellyfin_id FROM items WHERE kind = 'Episode' ORDER BY jellyfin_id LIMIT 1")
    episode = select("SELECT * FROM items WHERE jellyfin_id = ?", watched[0]["jellyfin_id"])[0]
    sibling = select(
        "SELECT jellyfin_id FROM items WHERE kind = 'Episode' AND season_id = ? AND jellyfin_id <> ? ORDER BY index_number LIMIT 1",
        episode["season_id"],
        episode["jellyfin_id"],
    )
    chosen = [playback_item, *(row["jellyfin_id"] for row in members + loner), episode["series_id"], episode["season_id"], episode["jellyfin_id"], *(row["jellyfin_id"] for row in sibling)]
    chosen = list(dict.fromkeys(chosen))
    items = [select("SELECT * FROM items WHERE jellyfin_id = ?", item)[0] for item in chosen]
    movie_tmdb = [int(item["tmdb_id"]) for item in items if item["kind"] == "Movie" and item["tmdb_id"]]

    memberships = select(
        f"SELECT * FROM franchise_movie_membership WHERE tmdb_id IN ({','.join('?' * len(movie_tmdb))}) ORDER BY tmdb_id",
        *movie_tmdb,
    )
    franchise_ids = sorted({row["collection_id"] for row in memberships if row["collection_id"] is not None})
    franchises = select(
        f"SELECT * FROM franchise_snapshots WHERE tmdb_collection_id IN ({','.join('?' * len(franchise_ids))}) ORDER BY tmdb_collection_id",
        *franchise_ids,
    )
    franchise_items = [
        row
        for franchise in franchise_ids
        for row in select(
            """SELECT * FROM franchise_snapshot_items WHERE tmdb_collection_id = ?
               AND (source_order < 2 OR tmdb_id IN (SELECT tmdb_id FROM franchise_movie_membership WHERE collection_id = ?))
               ORDER BY source_order""",
            franchise,
            franchise,
        )
        if row["source_order"] < 2 or row["tmdb_id"] in movie_tmdb
    ]
    snapshot = select("SELECT * FROM collection_snapshots ORDER BY committed_at LIMIT 1")[0]
    snapshot_items = select(
        "SELECT * FROM collection_snapshot_items WHERE profile_id = ? AND revision = ? ORDER BY source_order LIMIT 3",
        snapshot["profile_id"],
        snapshot["revision"],
    )
    ratings = select(
        f"SELECT * FROM rating_cache WHERE provider = 'tmdb' AND provider_id IN ({','.join('?' * len(movie_tmdb))}) ORDER BY provider_id",
        *(str(tmdb) for tmdb in movie_tmdb),
    ) + select(
        f"SELECT * FROM rating_cache WHERE provider_id NOT IN ({','.join('?' * len(movie_tmdb))}) ORDER BY provider, provider_id LIMIT 2",
        *(str(tmdb) for tmdb in movie_tmdb),
    )

    def account(row: dict[str, object]) -> dict[str, object]:
        return {**row, "server_id": fakes.hex_id(row["server_id"]), "user_id": fakes.hex_id(row["user_id"])}

    def json_list(text: str, kind: str) -> str:
        return json.dumps([fakes.label(kind, value) for value in json.loads(text)], separators=(",", ":"))

    def snapshot_item(row: dict[str, object]) -> dict[str, object]:
        title = fakes.label("Title", str(row["tmdb_id"]))
        return {
            **account(row),
            "tmdb_id": fakes.tmdb(row["tmdb_id"]),
            "title": title,
            "original_title": None if row["original_title"] is None else title,
            "overview": f"Overview of {title}.",
            "poster_path": fakes.artwork(row["poster_path"]),
            "backdrop_path": fakes.artwork(row["backdrop_path"]),
        }

    tables: list[tuple[str, list[dict[str, object]], str | None]] = []
    tables.append(("credentials", [{
        **credentials,
        "server_url": "http://jellyfin.invalid",
        "user_name": "user",
        "server_id": fakes.hex_id(credentials["server_id"]),
        "user_id": fakes.hex_id(credentials["user_id"]),
        "device_id": fakes.hex_id(credentials["device_id"]),
        "token": SYNTHETIC_TOKEN,
    }], "token: synthetic; 0.2.0's profile had its token removed"))
    meta = []
    for row in select("SELECT * FROM meta ORDER BY rowid"):
        value = row["value"]
        if row["key"] == "sync.watermark_ids":
            value = json.dumps([fakes.hex_id(item) for item in json.loads(value)], separators=(",", ":"))
        meta.append({"key": row["key"], "value": value})
    tables.append(("meta", meta, None))
    tables.append(("meta", [
        {"key": "cache.owner_server_id", "value": fakes.hex_id(server)},
        {"key": "cache.owner_user_id", "value": fakes.hex_id(user)},
    ], "synthetic: 0.2.0 writes these keys at sign-in, which the profile's seeded session skipped"))
    tables.append(("libraries", [
        {**row, "id": fakes.hex_id(row["id"]), "name": fakes.label("Library", str(row["name"])), "primary_image_tag": fakes.hex_id(row["primary_image_tag"])}
        for row in select("SELECT * FROM libraries ORDER BY position")
    ], None))
    sanitized_items = []
    for number, item in enumerate(items, start=1):
        name = fakes.label(str(item["kind"]), str(item["jellyfin_id"]))
        sanitized_items.append({
            **item,
            "id": number,
            "jellyfin_id": fakes.hex_id(item["jellyfin_id"]),
            "name": name,
            "original_title": None if item["original_title"] is None else name,
            "sort_name": None if item["sort_name"] is None else name.lower(),
            "parent_id": fakes.hex_id(item["parent_id"]),
            "series_id": fakes.hex_id(item["series_id"]),
            "series_name": None if item["series_id"] is None else fakes.label("Series", str(item["series_id"])),
            "season_id": fakes.hex_id(item["season_id"]),
            "tmdb_id": fakes.tmdb(item["tmdb_id"]),
            "imdb_id": fakes.imdb(item["imdb_id"]),
            "tvdb_id": fakes.tvdb(item["tvdb_id"]),
            "studios": json_list(str(item["studios"]), "Studio"),
            "tags": json_list(str(item["tags"]), "tag"),
            "image_tags": json.dumps({kind: fakes.hex_id(tag) for kind, tag in json.loads(str(item["image_tags"])).items()}, separators=(",", ":")),
            "primary_image_tag": fakes.hex_id(item["primary_image_tag"]),
            "backdrop_image_tag": fakes.hex_id(item["backdrop_image_tag"]),
        })
    tables.append(("items", sanitized_items, "inserting an item fills item_genres, item_studios, item_tags and items_fts through 0.2.0's triggers"))
    tables.append(("user_data", [
        {**row, "jellyfin_id": fakes.hex_id(row["jellyfin_id"])}
        for item in chosen
        for row in select("SELECT * FROM user_data WHERE jellyfin_id = ?", item)
    ], None))
    tables.append(("item_libraries", [
        {"library_id": fakes.hex_id(row["library_id"]), "item_id": fakes.hex_id(row["item_id"])}
        for item in chosen
        for row in select("SELECT * FROM item_libraries WHERE item_id = ? ORDER BY library_id", item)
    ], None))
    tables.append(("rating_cache", [
        {
            **row,
            "provider_id": fakes.tmdb(row["provider_id"]) if row["provider"] == "tmdb" else fakes.imdb(str(row["provider_id"])),
            "ratings": json.dumps(
                [{**entry, "votes": None if entry["votes"] is None else 1000} for entry in json.loads(str(row["ratings"]))],
                separators=(",", ":"),
            ),
        }
        for row in ratings
    ], None))
    tables.append(("collection_snapshots", [{
        **account(snapshot),
        "profile_id": fakes.hex_id(snapshot["profile_id"]),
        "revision": fakes.hex_id(snapshot["revision"]),
    }], None))
    tables.append(("collection_snapshot_items", [
        {**snapshot_item(row), "profile_id": fakes.hex_id(row["profile_id"]), "revision": fakes.hex_id(row["revision"])}
        for row in snapshot_items
    ], "children of collection_snapshots, deleted with it by ON DELETE CASCADE"))
    tables.append(("collection_refresh_state", [
        {**account(row), "profile_id": row["profile_id"] if not HEX_ID.match(str(row["profile_id"])) else fakes.hex_id(row["profile_id"])}
        for row in select("SELECT * FROM collection_refresh_state ORDER BY profile_id")
    ], None))
    tables.append(("franchise_snapshots", [
        {
            **account(row),
            "tmdb_collection_id": fakes.collection(row["tmdb_collection_id"]),
            "name": f"{fakes.label('Franchise', str(row['tmdb_collection_id']))} Collection",
            "poster_path": fakes.artwork(row["poster_path"]),
            "backdrop_path": fakes.artwork(row["backdrop_path"]),
        }
        for row in franchises
    ], None))
    tables.append(("franchise_snapshot_items", [
        {**snapshot_item(row), "tmdb_collection_id": fakes.collection(row["tmdb_collection_id"])}
        for row in franchise_items
    ], "children of franchise_snapshots, deleted with it by ON DELETE CASCADE"))
    tables.append(("franchise_movie_membership", [
        {**account(row), "tmdb_id": fakes.tmdb(row["tmdb_id"]), "collection_id": None if row["collection_id"] is None else fakes.collection(row["collection_id"])}
        for row in memberships
    ], None))
    tables.append(("provider_identity_map", [{
        "media_type": "series", "provider": "tvdb", "provider_id": fakes.tvdb("synthetic-series"),
        "tmdb_id": 999001, "resolved_at": credentials["updated_at"],
    }], "synthetic: the profile had no identity mapping"))
    letterboxd_profile = fakes.hex_id("0" * 31 + "1")
    tables.append(("external_profiles", [{
        "id": letterboxd_profile, "provider": "letterboxd", "profile_key": "fixture-member",
        "display_name": "Fixture Member", "canonical_url": "https://letterboxd.com/fixture-member/",
        "jellyfin_server_id": fakes.hex_id(server), "jellyfin_user_id": fakes.hex_id(user),
        "enabled": 1, "verification_status": "verified", "created_at": credentials["updated_at"], "last_checked_at": None,
    }], "synthetic: a Letterboxd profile staged by a schema-13 recreation, not yet imported into accounts.json"))
    tables.append(("legacy_item_playback_preferences", [{
        "jellyfin_id": fakes.hex_id(playback_item), "jellyfin_server_key": fakes.hex_id(server), "jellyfin_user_id": fakes.hex_id(user),
        "media_source": json.dumps({"index": 0, "name": "Main"}, separators=(",", ":")),
        "audio_track": json.dumps({"index": 1, "language": "jpn"}, separators=(",", ":")),
        "subtitle_track": None, "updated_at": credentials["updated_at"],
    }], "synthetic: a playback choice staged by a schema-13 recreation, not yet imported"))

    lines = [
        "-- Rows from the library.db the released MediaFlick Desktop 0.2.0 AppImage wrote on 2026-11-17,",
        "-- after a first sync of a real Jellyfin 12.2 library with the MediaFlick Companion. Frozen:",
        "-- never edit this file. Sanitized by scripts/fixtures/freeze_release.py: ids, titles, artwork",
        "-- paths and account names are fakes, mapped consistently. Load it after schema-23.sql.",
    ]
    for table, entries, note in tables:
        lines.append("")
        if note:
            lines.append(f"-- {table}: {note}")
        for entry in entries:
            columns = ", ".join(f'"{column}"' for column in entry)
            values = ", ".join(literal(value) for value in entry.values())
            lines.append(f'INSERT INTO "{table}" ({columns}) VALUES ({values});')
    return "\n".join(lines) + "\n"


def preferences(config: Path, fakes: Fakes) -> dict[str, str]:
    def load(name: str) -> dict:
        return json.loads((config / name).read_text(encoding="utf-8"))

    def accounts(document: dict, each) -> dict:
        return {
            **document,
            "accounts": [
                {**entry, "serverId": fakes.hex_id(entry["serverId"]), "userId": fakes.hex_id(entry["userId"]), **each(entry)}
                for entry in document["accounts"]
            ],
        }

    def account_settings(entry: dict) -> dict:
        browsing = dict(entry.get("browsing", {}))
        if "last" in browsing:
            browsing["last"] = re.sub(r"[0-9a-f]{32}", lambda match: fakes.hex_id(match.group()), browsing["last"])
        profiles = [
            {
                **profile,
                "id": fakes.hex_id(profile["id"]),
                "profileKey": "fixture-critic",
                "displayName": "Fixture Critic",
                "canonicalUrl": "https://letterboxd.com/fixture-critic/",
            }
            for profile in entry.get("letterboxd_profiles", [])
        ]
        return {"browsing": browsing, "letterboxd_profiles": profiles}

    def collection_settings(entry: dict) -> dict:
        return {
            "profiles": [
                {
                    **profile,
                    "id": fakes.hex_id(profile["id"]),
                    "revision": fakes.hex_id(profile["revision"]),
                    "customPosterId": fakes.hex_id(profile.get("customPosterId")),
                }
                for profile in entry["profiles"]
            ]
        }

    def playback_settings(entry: dict) -> dict:
        items = []
        for item in entry["items"]:
            name = fakes.label("Movie", item["itemId"])
            source = item["mediaSource"]
            items.append({
                **item,
                "itemId": fakes.hex_id(item["itemId"]),
                "mediaSource": {**source, "id": fakes.hex_id(source["id"]), "name": f"{name} - Remux-2160p"},
            })
        return {"items": items}

    def pretty(value: object) -> str:
        return json.dumps(value, indent=2, ensure_ascii=False)
    return {
        "settings.json": pretty(load("settings.json")),
        "accounts.json": pretty(accounts(load("accounts.json"), account_settings)),
        "collections.json": pretty(accounts(load("collections.json"), collection_settings)),
        "playback-preferences.json": pretty(accounts(load("playback-preferences.json"), playback_settings)),
    }


def open_read_only(database: Path) -> sqlite3.Connection:
    return sqlite3.connect(f"{database.resolve().as_uri()}?mode=ro&immutable=1", uri=True)


def main(argv: list[str]) -> int:
    if len(argv) not in (3, 4) or argv[1] not in ("schema", "0.2.0"):
        print(__doc__, file=sys.stderr)
        return 2
    out = Path(argv[3]) if len(argv) == 4 else Path("fixtures")
    if argv[1] == "schema":
        db = open_read_only(Path(argv[2]))
        version = db.execute("PRAGMA user_version").fetchone()[0]
        outputs = {out / "library-db" / "released" / f"schema-{version}.sql": schema(db)}
    else:
        profile = Path(argv[2])
        database = profile / "data" / "mediaflick-desktop" / "library.db"
        db = open_read_only(database)
        version = db.execute("PRAGMA user_version").fetchone()[0]
        if version != 23:
            raise SystemExit(f"{database} is at schema {version}, not 0.2.0's 23")
        fakes = Fakes()
        outputs = {
            out / "library-db" / "released" / "schema-23.sql": schema(db),
            out / "library-db" / "released" / "rows-23.sql": rows(db, fakes),
            **{
                out / "preferences" / "released" / "0.2.0" / name: text
                for name, text in preferences(profile / "config" / "mediaflick-desktop", fakes).items()
            },
        }
    for path, text in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
