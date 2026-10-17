use super::*;

#[test]
fn a_failed_library_list_still_fills_a_fresh_catalog() {
    let library = Arc::new(Library::open_in_memory().expect("library"));

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![ok(NOT_JSON), ok(ONE_MOVIE), ok(ONE_MOVIE)],
    );

    result.expect("the catalog synced without the library list");
    assert_eq!(steps(&requests), ["views", "bootstrap", "incremental"]);
    assert!(bootstrap_progress(&library).complete);
    assert!(ownership_available(&library));
    assert_eq!(listed(&library), None);
}

#[test]
fn a_failed_library_list_keeps_the_stored_libraries_and_membership() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(&library, &[(MOVIES, "movies", &["m1", "m2"])]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![ok(NOT_JSON), ok(EMPTY_PAGE)],
    );

    result.expect("the catalog synced without the library list");
    assert_eq!(steps(&requests), ["views", "incremental"]);
    assert_eq!(listed(&library), Some(vec![MOVIES.to_string()]));
    assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    assert!(ownership_available(&library));
}

#[test]
fn a_failed_membership_walk_keeps_its_membership_and_only_it_is_walked_again() {
    let library = settled_library(&[
        ("m1", "Movie"),
        ("m2", "Movie"),
        ("m3", "Movie"),
        ("s1", "Series"),
        ("s2", "Series"),
    ]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1", "m2"]),
            (SHOWS, "tvshows", &["s1"]),
        ],
    );
    let listing = || views(&[(MOVIES, "movies"), (SHOWS, "tvshows")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1", "m2", "m3", "s1", "s2"], 5),
            ok(NOT_JSON),
            members(&["s1", "s2"], 2),
        ],
    );
    result.expect("the catalog synced without one library's members");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies", "walk shows"]
    );
    assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    assert_eq!(grid(&library, SHOWS), ["s1", "s2"]);
    assert_eq!(walks_due(&library), [MOVIES]);
    assert!(ownership_available(&library));

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE), members(&["m1", "m2", "m3"], 3)],
    );
    result.expect("the next cycle");
    assert_eq!(steps(&requests), ["views", "incremental", "walk movies"]);
    assert_eq!(grid(&library, MOVIES), ["m1", "m2", "m3"]);
    assert!(walks_due(&library).is_empty());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn a_library_added_by_a_failed_cycle_is_walked_by_the_next() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    let listing = || views(&[(MOVIES, "movies"), (MUSIC, "music")]);

    let (result, requests) = cycle(&library, Trigger::Scheduled, vec![listing(), ok(NOT_JSON)]);
    assert!(matches!(result, Err(ApiError::Decode(_))), "{result:?}");
    assert_eq!(steps(&requests), ["views", "incremental"]);
    assert_eq!(walks_due(&library), [MOVIES]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE), members(&["m1", "m2"], 2)],
    );
    result.expect("the next cycle");
    assert_eq!(steps(&requests), ["views", "incremental", "walk movies"]);
    assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    assert!(walks_due(&library).is_empty());
}

#[test]
fn a_rate_limit_ends_the_membership_pass_and_a_lost_connection_skips_only_its_library() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1"]),
            (MORE_MOVIES, "movies", &["m2"]),
            (SHOWS, "tvshows", &["s1"]),
        ],
    );
    let listing = || {
        views(&[
            (MOVIES, "movies"),
            (MORE_MOVIES, "movies"),
            (SHOWS, "tvshows"),
        ])
    };
    let limited = || Reply::Answer(429, String::new());

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1", "m2", "s1"], 3),
            limited(),
            limited(),
            limited(),
        ],
    );
    result.expect("the catalog synced without the membership pass");
    assert_eq!(
        steps(&requests),
        [
            "views",
            "incremental",
            "sweep",
            "walk movies",
            "walk movies",
            "walk movies"
        ]
    );
    assert_eq!(walks_due(&library), [MOVIES, MORE_MOVIES, SHOWS]);
    assert_eq!(grid(&library, MOVIES), ["m1"]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
            Reply::Drop,
            Reply::Drop,
            Reply::Drop,
            members(&["s1"], 1),
        ],
    );
    result.expect("the catalog synced without one library's walk");
    assert_eq!(
        steps(&requests),
        [
            "views",
            "incremental",
            "walk movies",
            "walk more movies",
            "walk more movies",
            "walk more movies",
            "walk shows"
        ]
    );
    assert_eq!(walks_due(&library), [MORE_MOVIES]);
    assert_eq!(grid(&library, SHOWS), ["s1"]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE), members(&["m2"], 1)],
    );
    result.expect("the cycle that finishes the pass");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "walk more movies"]
    );
    assert!(walks_due(&library).is_empty());
    assert_eq!(grid(&library, MORE_MOVIES), ["m2"]);
}

#[test]
fn renaming_reordering_or_new_artwork_does_not_walk_membership() {
    let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
    );
    let relisted = json!({
        "Items": [
            { "Id": SHOWS, "Name": "tvshows", "CollectionType": "tvshows" },
            {
                "Id": MOVIES,
                "Name": "Films",
                "CollectionType": "movies",
                "ImageTags": { "Primary": "new-artwork" },
            },
        ],
        "TotalRecordCount": 2,
    });

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![ok(&relisted.to_string()), ok(EMPTY_PAGE)],
    );

    result.expect("cycle");
    assert_eq!(steps(&requests), ["views", "incremental"]);
    assert_eq!(
        listed(&library),
        Some(vec![SHOWS.to_string(), MOVIES.to_string()])
    );
    assert_eq!(grid(&library, MOVIES), ["m1"]);
}

#[test]
fn a_membership_walk_that_ends_short_of_its_total_keeps_the_previous_membership() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(&library, &[(MOVIES, "movies", &["m1", "m2"])]);
    let listing = || views(&[(MOVIES, "movies")]);
    let first_page = 1..1_000;
    let second_page = 900..1_400;
    let id = |n: usize| format!("x{n:04}");
    let first = std::iter::once("m1".to_string())
        .chain(first_page.map(id))
        .collect::<Vec<_>>();
    let second = second_page.map(id).collect::<Vec<_>>();
    let received = first.len() + second.len();
    let page = |ids: &[String]| {
        members(
            &ids.iter().map(String::as_str).collect::<Vec<_>>(),
            received,
        )
    };

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1", "m2"], 2),
            page(&first),
            page(&second),
        ],
    );
    result.expect("cycle");
    assert_eq!(
        steps(&requests),
        [
            "views",
            "incremental",
            "sweep",
            "walk movies",
            "walk movies"
        ]
    );
    assert_eq!(grid(&library, MOVIES), ["m1", "m2"]);
    assert!(
        requests[3].contains("SortBy=DateCreated%2CSortName"),
        "{}",
        requests[3]
    );

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![listing(), ok(EMPTY_PAGE), members(&["m1"], 1)],
    );
    result.expect("the next cycle");
    assert_eq!(steps(&requests), ["views", "incremental", "walk movies"]);
    assert_eq!(grid(&library, MOVIES), ["m1"]);
}
