//! The schema a fresh `library.db` is created with.

/// The complete current schema.
///
/// `items` is a thin catalog index: one row per Movie/Series/Season/Episode
/// with only what browsing, sorting, filtering, joining, and drawing cards
/// needs. Rich metadata — synopsis, cast, technical streams, critic ratings —
/// is fetched live from Jellyfin when a surface asks for it.
pub(super) const SCHEMA: &str = r#"
CREATE TABLE credentials (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    server_url   TEXT,
    user_id      TEXT,
    user_name    TEXT,
    server_id    TEXT,
    device_id    TEXT NOT NULL,
    token        TEXT,
    updated_at   INTEGER NOT NULL
);

CREATE TABLE items (
    id                  INTEGER PRIMARY KEY,
    jellyfin_id         TEXT NOT NULL UNIQUE,
    kind                TEXT NOT NULL,
    name                TEXT NOT NULL,
    original_title      TEXT,
    sort_name           TEXT,
    year                INTEGER,
    premiere_date       TEXT,
    runtime_ticks       INTEGER,
    community_rating    REAL,
    official_rating     TEXT,
    parent_id           TEXT,
    series_id           TEXT,
    series_name         TEXT,
    season_id           TEXT,
    index_number        INTEGER,
    parent_index_number INTEGER,
    child_count         INTEGER,
    tmdb_id             TEXT,
    imdb_id             TEXT,
    tvdb_id             TEXT,
    genres              TEXT NOT NULL DEFAULT '[]',
    studios             TEXT NOT NULL DEFAULT '[]',
    tags                TEXT NOT NULL DEFAULT '[]',
    image_tags          TEXT NOT NULL DEFAULT '{}',
    primary_image_tag   TEXT,
    backdrop_image_tag  TEXT,
    search_genres       TEXT NOT NULL DEFAULT '',
    date_created        TEXT,
    date_last_saved     TEXT,
    series_status       TEXT,
    is_virtual          INTEGER NOT NULL DEFAULT 0,
    synced_at           INTEGER NOT NULL
);

-- The items_kind_<sort> indexes are built from ItemSort in library/mod.rs.
CREATE INDEX items_series ON items (series_id, parent_index_number, index_number);
CREATE INDEX items_series_episodes
    ON items (series_id, is_virtual, date_created, jellyfin_id) WHERE kind = 'Episode';
CREATE INDEX items_parent ON items (parent_id);
CREATE INDEX items_season ON items (season_id) WHERE season_id IS NOT NULL;
CREATE INDEX items_tmdb ON items (tmdb_id) WHERE tmdb_id IS NOT NULL;
CREATE INDEX items_imdb ON items (imdb_id) WHERE imdb_id IS NOT NULL;
CREATE INDEX items_tvdb ON items (tvdb_id) WHERE tvdb_id IS NOT NULL;

-- `items.genres` normalized for lookup. Parsing the JSON of every row made the
-- genre list and each genre filter a full scan. Triggers keep it current, so no
-- write path can forget it.
CREATE TABLE item_genres (
    genre   TEXT NOT NULL,
    item_id INTEGER NOT NULL,
    PRIMARY KEY (genre, item_id)
) WITHOUT ROWID;
CREATE INDEX item_genres_item ON item_genres (item_id);

CREATE TRIGGER item_genres_insert AFTER INSERT ON items BEGIN
    INSERT OR IGNORE INTO item_genres (genre, item_id)
    SELECT value, new.id FROM json_each(new.genres) WHERE type = 'text' AND value <> '';
END;

CREATE TRIGGER item_genres_update AFTER UPDATE OF genres ON items
    WHEN old.genres IS NOT new.genres
BEGIN
    DELETE FROM item_genres WHERE item_id = old.id;
    INSERT OR IGNORE INTO item_genres (genre, item_id)
    SELECT value, new.id FROM json_each(new.genres) WHERE type = 'text' AND value <> '';
END;

CREATE TRIGGER item_genres_delete AFTER DELETE ON items BEGIN
    DELETE FROM item_genres WHERE item_id = old.id;
END;

-- `items.studios` and `items.tags`, normalized as `item_genres` is.
CREATE TABLE item_studios (
    studio  TEXT NOT NULL,
    item_id INTEGER NOT NULL,
    PRIMARY KEY (studio, item_id)
) WITHOUT ROWID;
CREATE INDEX item_studios_item ON item_studios (item_id);

CREATE TRIGGER item_studios_insert AFTER INSERT ON items BEGIN
    INSERT OR IGNORE INTO item_studios (studio, item_id)
    SELECT value, new.id FROM json_each(new.studios) WHERE type = 'text' AND value <> '';
END;

CREATE TRIGGER item_studios_update AFTER UPDATE OF studios ON items
    WHEN old.studios IS NOT new.studios
BEGIN
    DELETE FROM item_studios WHERE item_id = old.id;
    INSERT OR IGNORE INTO item_studios (studio, item_id)
    SELECT value, new.id FROM json_each(new.studios) WHERE type = 'text' AND value <> '';
END;

CREATE TRIGGER item_studios_delete AFTER DELETE ON items BEGIN
    DELETE FROM item_studios WHERE item_id = old.id;
END;

CREATE TABLE item_tags (
    tag     TEXT NOT NULL,
    item_id INTEGER NOT NULL,
    PRIMARY KEY (tag, item_id)
) WITHOUT ROWID;
CREATE INDEX item_tags_item ON item_tags (item_id);

CREATE TRIGGER item_tags_insert AFTER INSERT ON items BEGIN
    INSERT OR IGNORE INTO item_tags (tag, item_id)
    SELECT value, new.id FROM json_each(new.tags) WHERE type = 'text' AND value <> '';
END;

CREATE TRIGGER item_tags_update AFTER UPDATE OF tags ON items
    WHEN old.tags IS NOT new.tags
BEGIN
    DELETE FROM item_tags WHERE item_id = old.id;
    INSERT OR IGNORE INTO item_tags (tag, item_id)
    SELECT value, new.id FROM json_each(new.tags) WHERE type = 'text' AND value <> '';
END;

CREATE TRIGGER item_tags_delete AFTER DELETE ON items BEGIN
    DELETE FROM item_tags WHERE item_id = old.id;
END;

CREATE TABLE user_data (
    jellyfin_id             TEXT PRIMARY KEY,
    played                  INTEGER NOT NULL DEFAULT 0,
    play_count              INTEGER NOT NULL DEFAULT 0,
    playback_position_ticks INTEGER NOT NULL DEFAULT 0,
    is_favorite             INTEGER NOT NULL DEFAULT 0,
    played_percentage       REAL,
    last_played_date        TEXT,
    updated_at              INTEGER NOT NULL
);

CREATE INDEX user_data_resume ON user_data (last_played_date DESC)
    WHERE playback_position_ticks > 0;
CREATE INDEX user_data_favorite ON user_data (is_favorite) WHERE is_favorite = 1;

CREATE VIRTUAL TABLE items_fts USING fts5(
    name,
    original_title,
    search_genres,
    content='items',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER items_fts_insert AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, name, original_title, search_genres)
    VALUES (new.id, new.name, new.original_title, new.search_genres);
END;

CREATE TRIGGER items_fts_delete AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, name, original_title, search_genres)
    VALUES ('delete', old.id, old.name, old.original_title, old.search_genres);
END;

CREATE TRIGGER items_fts_update AFTER UPDATE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, name, original_title, search_genres)
    VALUES ('delete', old.id, old.name, old.original_title, old.search_genres);
    INSERT INTO items_fts(rowid, name, original_title, search_genres)
    VALUES (new.id, new.name, new.original_title, new.search_genres);
END;

CREATE TABLE libraries (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    collection_type   TEXT,
    position          INTEGER NOT NULL,
    primary_image_tag TEXT,
    membership        TEXT NOT NULL DEFAULT 'unwalked'
                      CHECK (membership IN ('unwalked', 'owed', 'current'))
);

CREATE TABLE item_libraries (
    library_id TEXT NOT NULL,
    item_id    TEXT NOT NULL,
    PRIMARY KEY (library_id, item_id)
) WITHOUT ROWID;
CREATE INDEX item_libraries_item ON item_libraries (item_id);

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Public integrations are user-associated application data, not process
-- configuration. A household sharing one desktop must never inherit another
-- Jellyfin user's connected profile.
CREATE TABLE external_profiles (
    id                  TEXT PRIMARY KEY,
    provider            TEXT NOT NULL,
    profile_key         TEXT NOT NULL,
    display_name        TEXT NOT NULL,
    canonical_url       TEXT NOT NULL,
    jellyfin_server_id  TEXT NOT NULL,
    jellyfin_user_id    TEXT NOT NULL,
    enabled             INTEGER NOT NULL DEFAULT 1,
    verification_status TEXT NOT NULL,
    created_at          INTEGER NOT NULL,
    last_checked_at     INTEGER,
    UNIQUE(provider, profile_key, jellyfin_server_id, jellyfin_user_id)
);
CREATE INDEX external_profiles_account
    ON external_profiles (jellyfin_server_id, jellyfin_user_id, provider);

-- Schema-13 playback rows cannot be copied into the account JSON files until
-- those stores open later in startup. This durable handoff survives a crash
-- between database recreation and the file imports.
CREATE TABLE legacy_item_playback_preferences (
    jellyfin_id         TEXT NOT NULL,
    jellyfin_server_key TEXT NOT NULL,
    jellyfin_user_id    TEXT NOT NULL,
    media_source        TEXT NOT NULL CHECK (json_valid(media_source)),
    audio_track         TEXT CHECK (audio_track IS NULL OR json_valid(audio_track)),
    subtitle_track      TEXT CHECK (subtitle_track IS NULL OR json_valid(subtitle_track)),
    updated_at          INTEGER NOT NULL,
    PRIMARY KEY (jellyfin_id, jellyfin_server_key, jellyfin_user_id)
);

-- Stable-provider rating cache.
--
-- Cache identity deliberately excludes Jellyfin IDs: when Jellyfin recreates
-- an item, its TMDB/IMDb identity can reuse the same durable result. Provider
-- credentials stay in Companion and never enter this database.
CREATE TABLE rating_cache (
    provider           TEXT NOT NULL,
    provider_id        TEXT NOT NULL,
    media_type         TEXT NOT NULL CHECK (media_type IN ('movie', 'show')),
    ratings            TEXT NOT NULL CHECK (json_valid(ratings)),
    source_updated_at  TEXT,
    fetched_at         INTEGER NOT NULL,
    stale_at           INTEGER NOT NULL,
    expires_at         INTEGER NOT NULL,
    schema_version     INTEGER NOT NULL,
    PRIMARY KEY (provider, provider_id, media_type)
);
CREATE INDEX rating_cache_expiry ON rating_cache (expires_at);

-- Collection configuration lives in collections.json. These tables contain
-- provider and ownership results that the app may discard and rebuild.
CREATE TABLE collection_snapshots (
    server_id       TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    profile_id      TEXT NOT NULL,
    revision        TEXT NOT NULL,
    committed_at    INTEGER NOT NULL,
    item_count      INTEGER NOT NULL,
    PRIMARY KEY (server_id, user_id, profile_id, revision)
);

CREATE TABLE collection_snapshot_items (
    server_id       TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    profile_id      TEXT NOT NULL,
    revision        TEXT NOT NULL,
    media_type      TEXT NOT NULL CHECK (media_type IN ('movie', 'series')),
    tmdb_id         INTEGER NOT NULL CHECK (tmdb_id > 0),
    title           TEXT NOT NULL,
    original_title  TEXT,
    year            INTEGER,
    overview        TEXT NOT NULL,
    release_date    TEXT,
    source_order    INTEGER NOT NULL,
    poster_path     TEXT,
    backdrop_path   TEXT,
    adult           INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, user_id, profile_id, revision, media_type, tmdb_id),
    FOREIGN KEY (server_id, user_id, profile_id, revision)
        REFERENCES collection_snapshots(server_id, user_id, profile_id, revision)
        ON DELETE CASCADE
);
CREATE INDEX collection_snapshot_order
    ON collection_snapshot_items (server_id, user_id, profile_id, revision, source_order);
CREATE INDEX collection_snapshot_identity
    ON collection_snapshot_items (server_id, user_id, media_type, tmdb_id);

CREATE TABLE collection_refresh_state (
    server_id       TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    profile_id      TEXT NOT NULL,
    last_attempt    INTEGER,
    last_success    INTEGER,
    latest_failure  TEXT,
    next_due        INTEGER,
    initialized     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, user_id, profile_id)
);
CREATE INDEX collection_refresh_due
    ON collection_refresh_state (server_id, user_id, next_due);

CREATE TABLE franchise_snapshots (
    server_id          TEXT NOT NULL,
    user_id            TEXT NOT NULL,
    tmdb_collection_id INTEGER NOT NULL CHECK (tmdb_collection_id > 0),
    name               TEXT NOT NULL,
    poster_path        TEXT,
    backdrop_path      TEXT,
    committed_at       INTEGER NOT NULL,
    PRIMARY KEY (server_id, user_id, tmdb_collection_id)
);

CREATE TABLE franchise_snapshot_items (
    server_id          TEXT NOT NULL,
    user_id            TEXT NOT NULL,
    tmdb_collection_id INTEGER NOT NULL,
    media_type         TEXT NOT NULL CHECK (media_type = 'movie'),
    tmdb_id            INTEGER NOT NULL CHECK (tmdb_id > 0),
    title              TEXT NOT NULL,
    original_title     TEXT,
    year               INTEGER,
    overview           TEXT NOT NULL,
    release_date       TEXT,
    source_order       INTEGER NOT NULL,
    poster_path        TEXT,
    backdrop_path      TEXT,
    adult              INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, user_id, tmdb_collection_id, tmdb_id),
    FOREIGN KEY (server_id, user_id, tmdb_collection_id)
        REFERENCES franchise_snapshots(server_id, user_id, tmdb_collection_id)
        ON DELETE CASCADE
);

-- TMDB movie membership changes rarely. Keeping negative lookups as NULL is
-- just as important as positive ones: otherwise every non-franchise movie is
-- fetched again after each library sync.
CREATE TABLE franchise_movie_membership (
    server_id      TEXT NOT NULL,
    user_id        TEXT NOT NULL,
    tmdb_id        INTEGER NOT NULL CHECK (tmdb_id > 0),
    collection_id  INTEGER CHECK (collection_id > 0),
    resolved_at    INTEGER NOT NULL,
    PRIMARY KEY (server_id, user_id, tmdb_id)
);
CREATE INDEX franchise_movie_membership_age
    ON franchise_movie_membership (server_id, user_id, resolved_at);
CREATE INDEX franchise_snapshot_order
    ON franchise_snapshot_items (server_id, user_id, tmdb_collection_id, source_order);
CREATE INDEX franchise_snapshot_identity
    ON franchise_snapshot_items (server_id, user_id, media_type, tmdb_id);

CREATE TABLE provider_identity_map (
    media_type      TEXT NOT NULL CHECK (media_type IN ('movie', 'series')),
    provider        TEXT NOT NULL CHECK (provider IN ('imdb', 'tvdb')),
    provider_id     TEXT NOT NULL,
    tmdb_id         INTEGER NOT NULL CHECK (tmdb_id > 0),
    resolved_at     INTEGER NOT NULL,
    PRIMARY KEY (media_type, provider, provider_id)
);
CREATE INDEX provider_identity_tmdb
    ON provider_identity_map (media_type, tmdb_id);

"#;
