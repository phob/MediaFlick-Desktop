use std::collections::HashSet;

use serde_json::json;

use super::{ItemQuery, Library, WatchFilter, civil_from_days};
use crate::library::test_support::{dto, seeded};
use crate::library::{
    CatalogKind, ItemSort, KindFilter, current_release_decade, release_decade_from_id,
};

#[test]
fn search_matches_genres_by_prefix() {
    let library = seeded();
    let by_genre = library
        .query(&ItemQuery {
            search: Some("acti".to_string()),
            limit: 10,
            ..Default::default()
        })
        .expect("query");
    assert_eq!(by_genre.total, 1);
    assert_eq!(by_genre.items[0].id, "m1");
}

#[test]
fn release_decade_is_bounded_and_composes_with_every_library_filter() {
    let library = Library::open_in_memory().expect("library");
    library
        .upsert_page(&[
            dto(
                r#"{"Id":"previous","Name":"Previous Decade","Type":"Movie","ProductionYear":1989,
                "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#,
            ),
            dto(
                r#"{"Id":"start","Name":"Decade Start","Type":"Movie","ProductionYear":1990,
                "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#,
            ),
            dto(
                r#"{"Id":"end","Name":"Decade End","Type":"Movie","ProductionYear":1999,
                "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#,
            ),
            dto(
                r#"{"Id":"next","Name":"Next Decade","Type":"Movie","ProductionYear":2000,
                "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#,
            ),
            dto(
                r#"{"Id":"watched","Name":"Watched","Type":"Movie","ProductionYear":1995,
                "Genres":["Action"],"UserData":{"Played":true,"IsFavorite":true}}"#,
            ),
            dto(
                r#"{"Id":"drama","Name":"Drama","Type":"Movie","ProductionYear":1995,
                "Genres":["Drama"],"UserData":{"Played":false,"IsFavorite":true}}"#,
            ),
            dto(
                r#"{"Id":"not-listed","Name":"Not Listed","Type":"Movie","ProductionYear":1995,
                "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":false}}"#,
            ),
            dto(r#"{"Id":"unknown","Name":"Unknown Year","Type":"Movie",
                "Genres":["Action"],"UserData":{"Played":false,"IsFavorite":true}}"#),
        ])
        .expect("seed");

    let page = library
        .query(&ItemQuery {
            kinds: KindFilter::one(CatalogKind::Movie),
            genre: Some("Action".to_string()),
            release_decade: Some(1990),
            watched: Some(WatchFilter::Unwatched),
            favorite: Some(true),
            limit: 1,
            ..Default::default()
        })
        .expect("combined filters");
    assert_eq!(page.total, 2);
    assert_eq!(page.items.len(), 1);

    let all = library
        .query(&ItemQuery {
            kinds: KindFilter::one(CatalogKind::Movie),
            genre: Some("Action".to_string()),
            release_decade: Some(1990),
            watched: Some(WatchFilter::Unwatched),
            favorite: Some(true),
            limit: 10,
            ..Default::default()
        })
        .expect("all combined filters");
    assert_eq!(
        all.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>(),
        HashSet::from(["start", "end"])
    );
}

#[test]
fn movie_and_series_decades_use_release_and_first_air_years() {
    let library = Library::open_in_memory().expect("library");
    library
        .upsert_page(&[
            dto(r#"{"Id":"movie","Name":"Film","Type":"Movie","ProductionYear":1999}"#),
            dto(r#"{"Id":"series","Name":"Show","Type":"Series",
                "PremiereDate":"2017-02-15T00:00:00.0000000Z"}"#),
        ])
        .expect("seed");

    let movies = library
        .query(&ItemQuery {
            kinds: KindFilter::one(CatalogKind::Movie),
            release_decade: Some(1990),
            limit: 10,
            ..Default::default()
        })
        .expect("movies");
    let series = library
        .query(&ItemQuery {
            kinds: KindFilter::one(CatalogKind::Series),
            release_decade: Some(2010),
            limit: 10,
            ..Default::default()
        })
        .expect("series");

    assert_eq!(movies.items[0].id, "movie");
    assert_eq!(series.items[0].id, "series");
    assert_eq!(series.items[0].year, Some(2017));
}

#[test]
fn release_decade_ids_are_standard_and_not_in_the_future() {
    let current = current_release_decade();
    assert_eq!(release_decade_from_id("1900"), Some(1900));
    assert_eq!(release_decade_from_id(&current.to_string()), Some(current));
    assert_eq!(release_decade_from_id("1995"), None);
    assert_eq!(release_decade_from_id("1890"), None);
    assert_eq!(release_decade_from_id(&(current + 10).to_string()), None);
}

#[test]
fn titles_without_a_server_sort_name_sort_by_their_name() {
    let library = Library::open_in_memory().expect("library");
    library
        .upsert_page(&[
            dto(r#"{"Id":"gamma","Name":"gamma","Type":"Movie"}"#),
            dto(r#"{"Id":"beta","Name":"Beta","Type":"Movie"}"#),
            dto(r#"{"Id":"alpha","Name":"Alpha","Type":"Movie","SortName":"alpha"}"#),
        ])
        .expect("seed");

    let ids = library
        .query(&ItemQuery {
            kinds: KindFilter::one(CatalogKind::Movie),
            sort: ItemSort::Name,
            limit: 10,
            ..Default::default()
        })
        .expect("query")
        .items
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, ["alpha", "beta", "gamma"]);
}

#[test]
fn continue_watching_lists_partially_played_items() {
    let rows = seeded().continue_watching(10).expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "m1");
    assert_eq!(rows[0].position_ticks, 600_000_000i64);
}

#[test]
fn recently_added_episodes_keep_the_newest_episode_of_each_series() {
    let library = Library::open_in_memory().expect("library");
    library
        .upsert_page(&[
            dto(
                r#"{"Id":"sev-1","Name":"One","Type":"Episode","SeriesId":"sev",
                "IndexNumber":1,"ParentIndexNumber":1,"DateCreated":"2024-01-01"}"#,
            ),
            dto(
                r#"{"Id":"silo-1","Name":"One","Type":"Episode","SeriesId":"silo",
                "IndexNumber":1,"ParentIndexNumber":1,"DateCreated":"2024-02-01"}"#,
            ),
            dto(
                r#"{"Id":"sev-2","Name":"Two","Type":"Episode","SeriesId":"sev",
                "IndexNumber":2,"ParentIndexNumber":1,"DateCreated":"2024-03-01"}"#,
            ),
            dto(
                r#"{"Id":"andor-1","Name":"One","Type":"Episode","SeriesId":"andor",
                "IndexNumber":1,"ParentIndexNumber":1,"DateCreated":"2023-12-01"}"#,
            ),
            dto(r#"{"Id":"movie","Name":"Film","Type":"Movie","DateCreated":"2024-04-01"}"#),
        ])
        .expect("seed");

    let ids = |limit| {
        library
            .recently_added_episodes(limit)
            .expect("rows")
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(10), ["sev-2", "silo-1", "andor-1"]);
    assert_eq!(ids(2), ["sev-2", "silo-1"]);
}

#[test]
fn children_returns_episodes_of_a_season_in_order() {
    let library = Library::open_in_memory().expect("library");
    library
        .upsert_page(&[
            dto(
                r#"{"Id":"e2","Name":"A Second","Type":"Episode","SeriesId":"s1",
                    "ParentId":"season1","SeasonId":"season1",
                    "IndexNumber":2,"ParentIndexNumber":1}"#,
            ),
            dto(
                r#"{"Id":"e1","Name":"Z First","Type":"Episode","SeriesId":"s1",
                    "ParentId":"season1","SeasonId":"season1",
                    "IndexNumber":1,"ParentIndexNumber":1}"#,
            ),
        ])
        .expect("seed");

    let ids = library
        .children("season1")
        .expect("children")
        .into_iter()
        .map(|row| row.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, ["e1", "e2"]);
}

#[test]
fn next_episode_follows_broadcast_order() {
    let library = seeded();
    let next = library.next_episode("e1").expect("next").expect("episode");
    assert_eq!(next.id, "e2");
    assert!(library.next_episode("e2").expect("next").is_none());
}

#[test]
fn item_detail_includes_genres_and_provider_ids() {
    let library = Library::open_in_memory().expect("library");
    library
        .upsert_page(&[dto(r#"{"Id":"m1","Name":"The Matrix","Type":"Movie",
                "OriginalTitle":"Matrix","Genres":["Action"],
                "ProviderIds":{"Tmdb":"603"}}"#)])
        .expect("seed");
    let detail = library.item("m1").expect("query").expect("item");
    assert_eq!(detail.genres[0], "Action");
    assert_eq!(detail.original_title.as_deref(), Some("Matrix"));
    assert_eq!(detail.provider_ids.tmdb.as_deref(), Some("603"));
    assert!(library.item("missing").expect("query").is_none());

    let wire = serde_json::to_value(&detail).expect("detail json");
    assert_eq!(wire["id"], "m1");
    assert_eq!(wire["kind"], "Movie");
    assert_eq!(wire["played"], false);
    assert_eq!(wire["genres"], json!(["Action"]));
    assert_eq!(
        wire["providerIds"],
        json!({ "tmdb": "603", "imdb": null, "tvdb": null })
    );
    assert!(wire.get("summary").is_none());
}

#[test]
fn genre_lookups_follow_items_through_updates() {
    let library = Library::open_in_memory().expect("library");
    library
        .upsert_page(&[
            dto(r#"{"Id":"a","Name":"A","Type":"Movie","Genres":["drama","Action",""]}"#),
            dto(r#"{"Id":"b","Name":"B","Type":"Series","Genres":["Action"]}"#),
        ])
        .expect("seed");
    assert_eq!(
        library.genres(None).expect("genres"),
        vec!["Action", "drama"],
        "ordered case-insensitively, without empty names"
    );

    library
        .upsert_page(&[dto(
            r#"{"Id":"a","Name":"A","Type":"Movie","Genres":["Comedy"]}"#,
        )])
        .expect("retag");
    assert_eq!(
        library.genres(None).expect("genres"),
        vec!["Action", "Comedy"]
    );
    let action = library
        .query(&ItemQuery {
            genre: Some("Action".to_string()),
            limit: 10,
            ..Default::default()
        })
        .expect("action");
    assert_eq!(action.total, 1);
    assert_eq!(action.items[0].id, "b");

    library.forget("b").expect("forget");
    assert_eq!(library.genres(None).expect("genres"), vec!["Comedy"]);
}

#[test]
fn typed_search_is_an_all_words_prefix_match_that_ignores_fts_syntax() {
    let library = seeded();
    let search = |text: &str| {
        library.query(&ItemQuery {
            search: Some(text.to_string()),
            limit: 10,
            ..Default::default()
        })
    };

    let matrix = search(r#"matr" -"#).expect("punctuation is not a syntax error");
    assert_eq!(matrix.total, 1);
    assert_eq!(matrix.items[0].id, "m1");
    assert_eq!(search("matrix OR arrival").expect("operator word").total, 0);
    assert_eq!(search("the arriv").expect("two words").total, 0);
}

#[test]
fn search_follows_renamed_and_removed_items() {
    let library = Library::open_in_memory().expect("library");
    library
        .upsert_page(&[dto(r#"{"Id":"a","Name":"The Matrix","Type":"Movie"}"#)])
        .expect("seed");
    library
        .upsert_page(&[dto(r#"{"Id":"a","Name":"Reloaded","Type":"Movie"}"#)])
        .expect("rename");
    let total = |text: &str| {
        library
            .query(&ItemQuery {
                search: Some(text.to_string()),
                limit: 10,
                ..Default::default()
            })
            .expect("search")
            .total
    };
    assert_eq!(total("matr"), 0, "the old name still matches");
    assert_eq!(total("reloa"), 1);

    library.forget("a").expect("forget");
    library
        .upsert_page(&[dto(r#"{"Id":"b","Name":"Arrival","Type":"Movie"}"#)])
        .expect("reuse");
    assert_eq!(total("reloa"), 0, "a removed name still matches");
    assert_eq!(total("arriv"), 1);
}

#[test]
fn civil_dates_convert_at_known_epochs() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(19_723), (2024, 1, 1));
}
