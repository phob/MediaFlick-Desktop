use std::collections::BTreeMap;
use std::path::PathBuf;

use proptest::prelude::*;
use rusqlite::{Connection, params};

use crate::library::catalog::{SET_LOCAL_PLAYED_SQL, UPSERT_ITEM_SQL, UPSERT_USER_DATA_SQL};

use super::configure;
use super::migrate::{FIRST_RELEASED_SCHEMA, SCHEMA_VERSION, migrate, user_version};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ExpectedSeriesAgg {
    latest_created: Option<String>,
    latest_played: Option<String>,
    unplayed_present: i64,
    started_episodes: i64,
}

#[derive(Debug, Clone)]
enum LegacyWriteOp {
    UpsertTopItem {
        slot: u8,
        is_series: bool,
        word_idx: u8,
        genre_idx: u8,
    },
    UpsertEpisode {
        ep_slot: u8,
        series_slot: u8,
        is_virtual: bool,
        date_idx: u8,
    },
    DeleteItem {
        target_id: u8,
    },
    UpsertUserData {
        target_id: u8,
        played: bool,
        ticks: i64,
        played_date_idx: Option<u8>,
    },
    SetLocalPlayed {
        target_id: u8,
        played: bool,
    },
    DeleteUserData {
        target_id: u8,
    },
}

fn legacy_op_strategy() -> impl Strategy<Value = LegacyWriteOp> {
    prop_oneof![
        (0u8..4, any::<bool>(), 0u8..4, 0u8..3).prop_map(
            |(slot, is_series, word_idx, genre_idx)| LegacyWriteOp::UpsertTopItem {
                slot,
                is_series,
                word_idx,
                genre_idx,
            }
        ),
        (0u8..6, 0u8..4, any::<bool>(), 0u8..4).prop_map(
            |(ep_slot, series_slot, is_virtual, date_idx)| LegacyWriteOp::UpsertEpisode {
                ep_slot,
                series_slot,
                is_virtual,
                date_idx,
            }
        ),
        (0u8..10).prop_map(|target_id| LegacyWriteOp::DeleteItem { target_id }),
        (
            0u8..10,
            any::<bool>(),
            0i64..300_000,
            prop::option::of(0u8..4)
        )
            .prop_map(|(target_id, played, ticks, played_date_idx)| {
                LegacyWriteOp::UpsertUserData {
                    target_id,
                    played,
                    ticks,
                    played_date_idx,
                }
            }),
        (0u8..10, any::<bool>())
            .prop_map(|(target_id, played)| LegacyWriteOp::SetLocalPlayed { target_id, played }),
        (0u8..10).prop_map(|target_id| LegacyWriteOp::DeleteUserData { target_id }),
    ]
}

fn released_v23_fixture() -> Connection {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/library-db/released");
    let schema = std::fs::read_to_string(dir.join(format!("schema-{FIRST_RELEASED_SCHEMA}.sql")))
        .expect("released schema");
    let rows = std::fs::read_to_string(dir.join(format!("rows-{FIRST_RELEASED_SCHEMA}.sql")))
        .expect("released rows");
    let conn = Connection::open_in_memory().expect("open");
    configure(&conn).expect("configure");
    conn.execute_batch(&schema).expect("schema");
    conn.execute_batch(&rows).expect("rows");
    conn.pragma_update(None, "user_version", FIRST_RELEASED_SCHEMA)
        .expect("stamp");
    migrate(&conn).expect("migrate to v24");
    assert_eq!(user_version(&conn).expect("version"), SCHEMA_VERSION);
    conn
}

fn target_jellyfin_id(idx: u8) -> String {
    if idx < 4 {
        format!("compat-top-{idx}")
    } else {
        format!("compat-ep-{}", idx - 4)
    }
}

fn assert_v24_derived_tables_match_recompute(conn: &Connection) {
    assert_v24_series_aggregates_match(conn);
    assert_v24_fts_matches(conn);
}

fn assert_v24_series_aggregates_match(conn: &Connection) {
    let mut agg_actual = BTreeMap::<String, ExpectedSeriesAgg>::new();
    let mut agg_stmt = conn
        .prepare(
            "SELECT series_id, latest_episode_created, latest_episode_played,
                    unplayed_present_count, started_episode_count
             FROM series_aggregates",
        )
        .expect("agg stmt");
    let agg_rows = agg_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ExpectedSeriesAgg {
                    latest_created: r.get(1)?,
                    latest_played: r.get(2)?,
                    unplayed_present: r.get(3)?,
                    started_episodes: r.get(4)?,
                },
            ))
        })
        .expect("agg query");
    for row in agg_rows {
        let (k, v) = row.expect("agg row");
        agg_actual.insert(k, v);
    }

    let mut agg_expected = BTreeMap::<String, ExpectedSeriesAgg>::new();
    let mut exp_stmt = conn
        .prepare(
            "SELECT e.series_id, e.is_virtual, e.date_created,
                    ue.played, ue.last_played_date, ue.playback_position_ticks
             FROM items e
             LEFT JOIN user_data ue ON ue.jellyfin_id = e.jellyfin_id
             WHERE e.kind = 'Episode' AND e.series_id IS NOT NULL",
        )
        .expect("exp stmt");
    let exp_rows = exp_stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? != 0,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?.unwrap_or(0) != 0,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<i64>>(5)?.unwrap_or(0),
            ))
        })
        .expect("exp query");
    for row in exp_rows {
        let (series_id, is_virtual, date_created, played, last_played, ticks) =
            row.expect("exp row");
        let entry = agg_expected.entry(series_id).or_default();
        if !is_virtual {
            if let Some(created) = date_created
                && entry
                    .latest_created
                    .as_ref()
                    .is_none_or(|cur| created > *cur)
            {
                entry.latest_created = Some(created);
            }
            if !played {
                entry.unplayed_present += 1;
            }
        }
        if let Some(played_at) = last_played
            && entry
                .latest_played
                .as_ref()
                .is_none_or(|cur| played_at > *cur)
        {
            entry.latest_played = Some(played_at);
        }
        if played || ticks > 0 {
            entry.started_episodes += 1;
        }
    }
    assert_eq!(agg_actual, agg_expected);
}

fn assert_v24_fts_matches(conn: &Connection) {
    let mut item_text_stmt = conn
        .prepare(
            "SELECT id, COALESCE(name, ''), COALESCE(original_title, ''), COALESCE(search_genres, '')
             FROM items
             WHERE kind IN ('Movie', 'Series')
             ORDER BY id",
        )
        .expect("item text stmt");
    let top_items: Vec<(i64, Vec<String>)> = item_text_stmt
        .query_map([], |r| {
            let id = r.get::<_, i64>(0)?;
            let combined = format!(
                "{} {} {}",
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?
            );
            let words: Vec<String> = combined
                .to_lowercase()
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(str::to_string)
                .collect();
            Ok((id, words))
        })
        .expect("item text query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("top items");

    let fts_tokens = ["alpha", "bravo", "cobalt", "delta", "noir", "sci", "drama"];
    for token in fts_tokens {
        let mut fts_exact_stmt = conn
            .prepare("SELECT rowid FROM catalog_search_fts WHERE catalog_search_fts MATCH ?1 ORDER BY rowid")
            .expect("fts exact stmt");
        let fts_exact = fts_exact_stmt
            .query_map([token], |r| r.get::<_, i64>(0))
            .expect("fts exact query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("fts exact ids");
        let expected_exact: Vec<i64> = top_items
            .iter()
            .filter(|(_, words)| words.iter().any(|w| w == token))
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(
            fts_exact, expected_exact,
            "FTS exact mismatch for token {token}"
        );

        let prefix_query = format!("{token}*");
        let mut fts_prefix_stmt = conn
            .prepare("SELECT rowid FROM catalog_search_fts WHERE catalog_search_fts MATCH ?1 ORDER BY rowid")
            .expect("fts prefix stmt");
        let fts_prefix = fts_prefix_stmt
            .query_map([&prefix_query], |r| r.get::<_, i64>(0))
            .expect("fts prefix query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("fts prefix ids");
        let expected_prefix: Vec<i64> = top_items
            .iter()
            .filter(|(_, words)| words.iter().any(|w| w.starts_with(token)))
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(
            fts_prefix, expected_prefix,
            "FTS prefix mismatch for token {prefix_query}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        cases: 24,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_v0_2_0_sql_writes_into_v24_keep_aggregates_and_fts_exact(
        ops in prop::collection::vec(legacy_op_strategy(), 1..40)
    ) {
        let conn = released_v23_fixture();
        let words = ["Alpha", "Bravo", "Cobalt", "Delta"];
        let genres = ["Noir", "Sci Fi", "Drama"];
        let dates = [
            "2026-01-01T00:00:00Z",
            "2026-02-01T00:00:00Z",
            "2026-03-01T00:00:00Z",
            "2026-04-01T00:00:00Z",
        ];

        for (step, op) in ops.into_iter().enumerate() {
            let now = i64::try_from(step).unwrap_or(0) + 1000;
            match op {
                LegacyWriteOp::UpsertTopItem {
                    slot,
                    is_series,
                    word_idx,
                    genre_idx,
                } => {
                    let id = format!("compat-top-{slot}");
                    let kind = if is_series { "Series" } else { "Movie" };
                    let word = words[usize::from(word_idx) % words.len()];
                    let genre = genres[usize::from(genre_idx) % genres.len()];
                    conn.execute(
                        UPSERT_ITEM_SQL,
                        params![
                            id,
                            kind,
                            format!("{word} Title"),
                            format!("{word} Orig"),
                            word,
                            2024_i64,
                            None::<String>,
                            Some(60_000_000_i64),
                            None::<f64>,
                            None::<String>,
                            None::<String>,
                            None::<String>,
                            None::<String>,
                            None::<String>,
                            None::<i64>,
                            None::<i64>,
                            None::<i64>,
                            None::<String>,
                            None::<String>,
                            None::<String>,
                            serde_json::json!([genre]).to_string(),
                            "[]",
                            "[]",
                            "{}",
                            None::<String>,
                            None::<String>,
                            genre,
                            dates[0],
                            dates[0],
                            None::<String>,
                            0_i64,
                            now,
                        ],
                    )
                    .expect("upsert top item");
                }
                LegacyWriteOp::UpsertEpisode {
                    ep_slot,
                    series_slot,
                    is_virtual,
                    date_idx,
                } => {
                    let id = format!("compat-ep-{ep_slot}");
                    let series_id = format!("compat-top-{series_slot}");
                    let created = dates[usize::from(date_idx) % dates.len()];
                    conn.execute(
                        UPSERT_ITEM_SQL,
                        params![
                            id,
                            "Episode",
                            format!("Ep {ep_slot}"),
                            None::<String>,
                            "Ep",
                            None::<i64>,
                            None::<String>,
                            None::<i64>,
                            None::<f64>,
                            None::<String>,
                            None::<String>,
                            series_id,
                            "Series",
                            None::<String>,
                            Some(i64::from(ep_slot)),
                            Some(1_i64),
                            None::<i64>,
                            None::<String>,
                            None::<String>,
                            None::<String>,
                            "[]",
                            "[]",
                            "[]",
                            "{}",
                            None::<String>,
                            None::<String>,
                            "",
                            created,
                            created,
                            None::<String>,
                            i64::from(is_virtual),
                            now,
                        ],
                    )
                    .expect("upsert episode");
                }
                LegacyWriteOp::DeleteItem { target_id } => {
                    conn.execute(
                        "DELETE FROM items WHERE jellyfin_id = ?1",
                        params![target_jellyfin_id(target_id)],
                    )
                    .expect("delete item");
                }
                LegacyWriteOp::UpsertUserData {
                    target_id,
                    played,
                    ticks,
                    played_date_idx,
                } => {
                    let played_date = played_date_idx.map(|i| dates[usize::from(i) % dates.len()]);
                    conn.execute(
                        UPSERT_USER_DATA_SQL,
                        params![
                            target_jellyfin_id(target_id),
                            i64::from(played),
                            i64::from(played),
                            ticks,
                            0_i64,
                            None::<f64>,
                            played_date,
                            now,
                        ],
                    )
                    .expect("upsert user_data");
                }
                LegacyWriteOp::SetLocalPlayed { target_id, played } => {
                    conn.execute(
                        SET_LOCAL_PLAYED_SQL,
                        params![target_jellyfin_id(target_id), i64::from(played), i64::from(played), now],
                    )
                    .expect("set local played");
                }
                LegacyWriteOp::DeleteUserData { target_id } => {
                    conn.execute(
                        "DELETE FROM user_data WHERE jellyfin_id = ?1",
                        params![target_jellyfin_id(target_id)],
                    )
                    .expect("delete user_data");
                }
            }
            assert_v24_derived_tables_match_recompute(&conn);
        }
    }
}
