use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::{Value, json};

use super::{Facet, ItemQuery, Library};
use crate::library::libraries::LibraryId;
use crate::library::model::UserDataRecord;
use crate::library::test_support::dto;
use crate::library::{CatalogKind, ItemSort, KindFilter, TitleKind};

#[derive(Debug, Default, PartialEq, Eq)]
struct SeriesAggregateRow {
    latest_created: Option<String>,
    latest_played: Option<String>,
    unplayed_present: i64,
    started_episodes: i64,
}

fn max_opt(slot: &mut Option<String>, candidate: Option<String>) {
    if let Some(value) = candidate
        && slot.as_ref().is_none_or(|current| value > *current)
    {
        *slot = Some(value);
    }
}

fn assert_series_aggregates_match_truth(library: &Library) {
    library
        .with_connection(|connection| {
            let mut ep_stmt = connection.prepare(
                "SELECT e.series_id, e.is_virtual, e.date_created,
                        COALESCE(ue.played, 0), ue.last_played_date, COALESCE(ue.playback_position_ticks, 0)
                 FROM items e
                 LEFT JOIN user_data ue ON ue.jellyfin_id = e.jellyfin_id
                 WHERE e.kind = 'Episode' AND e.series_id IS NOT NULL",
            )?;
            let mut expected = std::collections::BTreeMap::<String, SeriesAggregateRow>::new();
            let mut rows = ep_stmt.query([])?;
            while let Some(row) = rows.next()? {
                let series_id: String = row.get(0)?;
                let is_virtual = row.get::<_, i64>(1)? != 0;
                let date_created: Option<String> = row.get(2)?;
                let played = row.get::<_, i64>(3)? != 0;
                let last_played: Option<String> = row.get(4)?;
                let ticks: i64 = row.get(5)?;
                let entry = expected.entry(series_id).or_default();
                if !is_virtual {
                    max_opt(&mut entry.latest_created, date_created);
                    if !played {
                        entry.unplayed_present += 1;
                    }
                }
                max_opt(&mut entry.latest_played, last_played);
                if played || ticks > 0 {
                    entry.started_episodes += 1;
                }
            }

            let mut agg_stmt = connection.prepare(
                "SELECT series_id, latest_episode_created, latest_episode_played,
                        unplayed_present_count, started_episode_count
                 FROM series_aggregates",
            )?;
            let mut actual = std::collections::BTreeMap::<String, SeriesAggregateRow>::new();
            let mut agg_rows = agg_stmt.query([])?;
            while let Some(row) = agg_rows.next()? {
                actual.insert(
                    row.get(0)?,
                    SeriesAggregateRow {
                        latest_created: row.get(1)?,
                        latest_played: row.get(2)?,
                        unplayed_present: row.get(3)?,
                        started_episodes: row.get(4)?,
                    },
                );
            }

            assert_eq!(actual, expected);
            Ok(())
        })
        .expect("series_aggregates check");
}

fn ep_user_data(id: &str, played: bool, ticks: i64, last_played: Option<&str>) -> UserDataRecord {
    UserDataRecord {
        jellyfin_id: id.to_string(),
        played,
        play_count: i64::from(played),
        playback_position_ticks: ticks,
        is_favorite: false,
        played_percentage: played.then_some(100.0),
        last_played_date: last_played.map(str::to_string),
    }
}

#[test]
fn series_aggregates_stay_in_sync_across_item_and_user_data_mutations() {
    let library = Library::open_in_memory().expect("open");
    let live = crate::jellyfin::session::Live::for_test();

    library
        .apply_user_data(
            &live,
            &[ep_user_data(
                "ep-pre",
                true,
                0,
                Some("2026-10-01T00:00:00Z"),
            )],
        )
        .expect("pre-seed user data");

    let items = vec![
        dto(r#"{"Id":"s1","Type":"Series","Name":"Show 1","DateCreated":"2026-01-01T00:00:00Z"}"#),
        dto(
            r#"{"Id":"ep-pre","Type":"Episode","Name":"Pre","SeriesId":"s1","SeriesName":"Show 1","ParentIndexNumber":1,"IndexNumber":1,"DateCreated":"2026-02-01T00:00:00Z"}"#,
        ),
        dto(
            r#"{"Id":"ep-2","Type":"Episode","Name":"Second","SeriesId":"s1","SeriesName":"Show 1","ParentIndexNumber":1,"IndexNumber":2,"DateCreated":"2026-03-01T00:00:00Z"}"#,
        ),
        dto(
            r#"{"Id":"ep-virt","Type":"Episode","Name":"Upcoming","SeriesId":"s1","SeriesName":"Show 1","ParentIndexNumber":1,"IndexNumber":3,"LocationType":"Virtual","DateCreated":"2026-09-01T00:00:00Z"}"#,
        ),
        dto(r#"{"Id":"s2","Type":"Series","Name":"Show 2","DateCreated":"2026-01-01T00:00:00Z"}"#),
    ];
    library.ingest_page(&live, &items).expect("ingest");
    assert_series_aggregates_match_truth(&library);

    library
        .apply_user_data(
            &live,
            &[
                ep_user_data("ep-2", false, 500_000, Some("2026-11-15T00:00:00Z")),
                ep_user_data("ep-pre", false, 0, Some("2026-09-15T00:00:00Z")),
            ],
        )
        .expect("update user data");
    assert_series_aggregates_match_truth(&library);

    library
        .apply_user_data(
            &live,
            &[ep_user_data("ep-2", true, 0, Some("2026-08-01T00:00:00Z"))],
        )
        .expect("regress last_played_date");
    assert_series_aggregates_match_truth(&library);

    library
        .ingest_page(
            &live,
            &[dto(
                r#"{"Id":"ep-2","Type":"Episode","Name":"Moved","SeriesId":"s2","SeriesName":"Show 2","ParentIndexNumber":1,"IndexNumber":1,"DateCreated":"2026-04-01T00:00:00Z"}"#,
            )],
        )
        .expect("move episode across series");
    assert_series_aggregates_match_truth(&library);

    library.forget(&live, "ep-2").expect("forget ep-2");
    assert_series_aggregates_match_truth(&library);

    library
        .with_connection(|connection| {
            connection.execute("DELETE FROM user_data WHERE jellyfin_id = 'ep-pre'", [])?;
            Ok(())
        })
        .expect("delete user data");
    assert_series_aggregates_match_truth(&library);

    library.forget(&live, "ep-pre").expect("forget ep-pre");
    library.forget(&live, "ep-virt").expect("forget ep-virt");
    assert_series_aggregates_match_truth(&library);
}

#[test]
fn play_count_fast_path_matches_full_sort_order() {
    let library = Library::open_in_memory().expect("open");
    let live = crate::jellyfin::session::Live::for_test();
    let movies = vec![
        dto(r#"{"Id":"m1","Type":"Movie","Name":"Alpha"}"#),
        dto(r#"{"Id":"m2","Type":"Movie","Name":"Beta"}"#),
        dto(r#"{"Id":"m3","Type":"Movie","Name":"Gamma"}"#),
        dto(r#"{"Id":"m4","Type":"Movie","Name":"Delta"}"#),
    ];
    library.ingest_page(&live, &movies).expect("ingest");
    library
        .apply_user_data(
            &live,
            &[
                UserDataRecord {
                    jellyfin_id: "m1".to_string(),
                    played: true,
                    play_count: 1,
                    playback_position_ticks: 0,
                    is_favorite: false,
                    played_percentage: Some(100.0),
                    last_played_date: None,
                },
                UserDataRecord {
                    jellyfin_id: "m2".to_string(),
                    played: true,
                    play_count: 5,
                    playback_position_ticks: 0,
                    is_favorite: false,
                    played_percentage: Some(100.0),
                    last_played_date: None,
                },
                UserDataRecord {
                    jellyfin_id: "m3".to_string(),
                    played: true,
                    play_count: 1,
                    playback_position_ticks: 0,
                    is_favorite: false,
                    played_percentage: Some(100.0),
                    last_played_date: None,
                },
            ],
        )
        .expect("user data");

    let page = library
        .query(&ItemQuery {
            kinds: KindFilter::one(TitleKind::Movie),
            sort: ItemSort::PlayCount,
            limit: 2,
            offset: 0,
            ..ItemQuery::default()
        })
        .expect("query");
    let ids: Vec<&str> = page.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, vec!["m2", "m1"]);
    assert_eq!(page.total, 4);
}

fn quantile_ms(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn measure_op<T>(warmup: usize, iters: usize, mut op: impl FnMut() -> T) -> (Value, f64) {
    for _ in 0..warmup {
        let _ = std::hint::black_box(op());
    }
    let mut samples_ms = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        let _ = std::hint::black_box(op());
        samples_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples_ms.sort_by(f64::total_cmp);
    let p50 = quantile_ms(&samples_ms, 0.50);
    let p99 = quantile_ms(&samples_ms, 0.99);
    let max_ms = samples_ms.last().copied().unwrap_or(0.0);
    (
        json!({
            "p50_ms": (p50 * 1000.0).round() / 1000.0,
            "p99_ms": (p99 * 1000.0).round() / 1000.0,
            "max_ms": (max_ms * 1000.0).round() / 1000.0,
        }),
        p99,
    )
}

const TARGET_SCHEMA_VERSION: i32 = 24;

fn open_migrated_100k(source: &Path, db_path: &Path) -> (Arc<Library>, f64) {
    std::fs::copy(source, db_path).expect("copy 100k fixture");
    {
        let raw = Connection::open(db_path).expect("raw open");
        let uv: i32 = raw
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user_version");
        if uv == 0 {
            raw.pragma_update(None, "user_version", 23)
                .expect("stamp released v23");
        }
    }
    let mig_start = Instant::now();
    let library = Arc::new(Library::open(db_path).expect("open upgraded 100k library"));
    let migration_ms = mig_start.elapsed().as_secs_f64() * 1000.0;
    library
        .with_connection(|connection| {
            let uv: i32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            assert_eq!(uv, TARGET_SCHEMA_VERSION);
            let series_aggs: i64 =
                connection.query_row("SELECT count(*) FROM series_aggregates", [], |row| {
                    row.get(0)
                })?;
            assert_eq!(series_aggs, 2500);
            Ok(())
        })
        .expect("verify migrated state");
    (library, migration_ms)
}

fn spawn_wal_writer(db_path: PathBuf, stop_writer: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let writer_lib = Library::open(&db_path).expect("writer open");
        let live = crate::jellyfin::session::Live::for_test();
        let mut tick = 1000_i64;
        while !stop_writer.load(Ordering::Relaxed) {
            tick += 100;
            let _ = writer_lib.apply_user_data(
                &live,
                &[UserDataRecord {
                    jellyfin_id: format!("movie-{:06}", tick % 1000),
                    played: (tick % 2) == 0,
                    play_count: 1,
                    playback_position_ticks: tick,
                    is_favorite: (tick % 5) == 0,
                    played_percentage: Some(50.0),
                    last_played_date: Some("2026-11-27T20:00:00Z".to_string()),
                }],
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    })
}

fn record_100k_case<'a>(
    report_rows: &mut serde_json::Map<String, Value>,
    failures: &mut Vec<String>,
    name: &'static str,
    op: Box<dyn FnMut() -> usize + 'a>,
) {
    let (stats, p99) = measure_op(5, 100, op);
    println!("{name}: {stats}");
    if p99 >= 16.0 {
        failures.push(format!("{name}: p99={p99:.3}ms >= 16.0ms"));
    }
    report_rows.insert(name.to_string(), stats);
}

fn run_100k_query_cases(
    library: &Library,
    report_rows: &mut serde_json::Map<String, Value>,
    failures: &mut Vec<String>,
) {
    let lib_movies = LibraryId::for_test("lib-movies");
    let lib_shows = LibraryId::for_test("lib-shows");
    let q_movies_name = ItemQuery {
        kinds: KindFilter::one(TitleKind::Movie),
        library: Some(lib_movies),
        sort: ItemSort::Name,
        limit: 60,
        offset: 0,
        ..ItemQuery::default()
    };
    let q_movies_p100 = ItemQuery {
        offset: 6000,
        ..q_movies_name.clone()
    };
    let q_movies_random = ItemQuery {
        sort: ItemSort::Random { seed: 42 },
        ..q_movies_name.clone()
    };
    let q_movies_play_count = ItemQuery {
        sort: ItemSort::PlayCount,
        ..q_movies_name.clone()
    };
    let q_movies_favorites = ItemQuery {
        favorite: Some(true),
        sort: ItemSort::DateAdded,
        library: None,
        ..q_movies_name.clone()
    };
    let q_series_ep_added = ItemQuery {
        kinds: KindFilter::one(TitleKind::Series),
        library: Some(lib_shows),
        sort: ItemSort::EpisodeAdded,
        limit: 60,
        ..ItemQuery::default()
    };
    let q_series_date_played = ItemQuery {
        sort: ItemSort::DatePlayed,
        ..q_series_ep_added.clone()
    };
    let q_fts_1char = ItemQuery {
        search: Some("a".to_string()),
        kinds: KindFilter::from_kinds(vec![CatalogKind::Movie, CatalogKind::Series]),
        limit: 60,
        ..ItemQuery::default()
    };

    let mut run = |name, query: &ItemQuery| {
        record_100k_case(
            report_rows,
            failures,
            name,
            Box::new(|| library.query(query).expect("q").items.len()),
        );
    };
    run("browse_series_date_played", &q_series_date_played);
    run("browse_movies_random", &q_movies_random);
    run("browse_movies_count_and_p0", &q_movies_name);
    run("favorites_date_added", &q_movies_favorites);
    run("browse_movies_play_count", &q_movies_play_count);
    run("fts_search_1char_with_count", &q_fts_1char);
    run("browse_series_ep_added", &q_series_ep_added);
    run("browse_movies_name_p100", &q_movies_p100);
    record_100k_case(
        report_rows,
        failures,
        "letter_index_movies",
        Box::new(|| library.letter_index(&q_movies_name).expect("li").len()),
    );
    record_100k_case(
        report_rows,
        failures,
        "fts_search_1char_page",
        Box::new(|| library.query_page(&q_fts_1char).expect("q").len()),
    );
}

fn run_100k_benchmarks(library: &Library) -> (serde_json::Map<String, Value>, Vec<String>) {
    let lib_movies = LibraryId::for_test("lib-movies");
    let mut report_rows = serde_json::Map::new();
    let mut failures = Vec::new();
    run_100k_query_cases(library, &mut report_rows, &mut failures);

    let mut run_extra = |name, op| record_100k_case(&mut report_rows, &mut failures, name, op);
    run_extra(
        "random_played_movie_with_genre",
        Box::new(|| {
            usize::from(
                library
                    .random_played_movie_with_genre()
                    .expect("q")
                    .is_some(),
            )
        }),
    );
    run_extra(
        "random_billboard_titles",
        Box::new(|| library.random_billboard_titles(12).expect("q").len()),
    );
    run_extra(
        "official_ratings_library",
        Box::new(|| {
            library
                .official_ratings(&[CatalogKind::Movie], Some(&lib_movies))
                .expect("q")
                .len()
        }),
    );
    run_extra(
        "facet_genre_library",
        Box::new(|| {
            library
                .facet_values(Facet::Genre, Some(&lib_movies))
                .expect("q")
                .len()
        }),
    );
    run_extra(
        "facet_studio_library",
        Box::new(|| {
            library
                .facet_values(Facet::Studio, Some(&lib_movies))
                .expect("q")
                .len()
        }),
    );
    run_extra(
        "facet_tag_library",
        Box::new(|| {
            library
                .facet_values(Facet::Tag, Some(&lib_movies))
                .expect("q")
                .len()
        }),
    );
    run_extra(
        "continue_watching",
        Box::new(|| library.continue_watching(24).expect("q").len()),
    );
    (report_rows, failures)
}

struct BenchScratch(PathBuf);

impl Drop for BenchScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore]
fn bench_100k_catalog_under_concurrent_wal() {
    let source = PathBuf::from("/tmp/mf-phase1-baseline/library-100k.db");
    assert!(
        source.exists(),
        "missing 100k fixture at {}",
        source.display()
    );

    let scratch = BenchScratch(
        std::env::temp_dir().join(format!("mf-100k-bench-{}", crate::app::ids::random_hex(6))),
    );
    std::fs::create_dir_all(&scratch.0).expect("mkdir");
    let db_path = scratch.0.join("library.db");
    let (library, migration_ms) = open_migrated_100k(&source, &db_path);

    let stop_writer = Arc::new(AtomicBool::new(false));
    let writer_handle = spawn_wal_writer(db_path, Arc::clone(&stop_writer));
    let (report_rows, failures) = run_100k_benchmarks(&library);
    stop_writer.store(true, Ordering::Relaxed);
    let _ = writer_handle.join();

    let out_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("build/e2e/deepen-player-spine/sqlite-100k-report.json");
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir report dir");
    }
    let report = json!({
        "migration_23_to_24_ms": (migration_ms * 100.0).round() / 100.0,
        "schema_version": TARGET_SCHEMA_VERSION,
        "queries": report_rows,
        "failures": failures,
    });
    let mut file = std::fs::File::create(&out_path).expect("create report");
    file.write_all(
        serde_json::to_string_pretty(&report)
            .expect("json")
            .as_bytes(),
    )
    .expect("write report");

    assert!(
        failures.is_empty(),
        "queries exceeded 16ms p99: {:?}",
        failures
    );
}
