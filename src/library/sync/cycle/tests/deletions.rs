use super::*;

#[test]
fn an_empty_answer_keeps_a_library_s_titles_and_settles_its_walk() {
    let library = settled_library(&[("m1", "Movie")]);
    sync_libraries(&library, &[(MOVIES, "movies", &["m1"])]);
    let listing = || views(&[(MOVIES, "movies")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![
            listing(),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
            members(&[], 0),
        ],
    );
    result.expect("cycle");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
    assert_eq!(grid(&library, MOVIES), ["m1"]);
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
fn an_owed_sweep_that_lists_nothing_stays_owed_until_its_deletion_runs() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1"]),
            (MORE_MOVIES, "movies", &["m2"]),
        ],
    );
    let remaining = || views(&[(MOVIES, "movies")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![remaining(), ok(EMPTY_PAGE), ok(EMPTY_PAGE)],
    );
    let report = result.expect("a cycle whose sweep listed nothing");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert!(library.item("m2").expect("query").is_some());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            remaining(),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
            members(&["m1"], 1),
        ],
    );
    let report = result.expect("the next cycle");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
    assert_eq!(report.deleted, 1);
    assert!(library.item("m2").expect("query").is_none());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![remaining(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn a_server_that_lists_no_library_and_no_title_empties_the_catalog() {
    let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
    );

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![views(&[]), ok(EMPTY_PAGE), ok(UNCOUNTED)],
    );
    let report = result.expect("a cycle whose sweep answer carries no count");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert_eq!(library.stats().total, 2);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![views(&[]), ok(EMPTY_PAGE), ok(EMPTY_PAGE)],
    );
    let report = result.expect("cycle");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 2);
    assert_eq!(library.stats().total, 0);
    assert_eq!(listed(&library), Some(Vec::new()));

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![views(&[]), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn removing_the_last_catalog_library_empties_the_catalog_while_music_remains() {
    let library = settled_library(&[("m1", "Movie")]);
    let (result, _) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            views(&[(MOVIES, "movies"), (MUSIC, "music")]),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
        ],
    );
    result.expect("libraries synced");
    let music_only = || views(&[(MUSIC, "music")]);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![music_only(), ok(EMPTY_PAGE), ok(UNCOUNTED)],
    );
    let report = result.expect("a cycle whose sweep answer carries no count");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert!(library.item("m1").expect("query").is_some());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![music_only(), ok(EMPTY_PAGE), ok(EMPTY_PAGE)],
    );
    let report = result.expect("a cycle whose sweep counts no title");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 1);
    assert_eq!(library.stats().total, 0);
    assert_eq!(listed(&library), Some(vec![MUSIC.to_string()]));

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![music_only(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn an_empty_answer_waits_for_confirmation_when_the_library_list_failed() {
    let library = settled_library(&[("m1", "Movie")]);
    let failed_list = || vec![ok(NOT_JSON), ok(EMPTY_PAGE), ok(EMPTY_PAGE)];

    let (result, requests) = cycle(&library, Trigger::Requested, failed_list());
    let report = result.expect("a cycle before the library list is known");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);

    let (result, requests) = cycle(
        &library,
        Trigger::Requested,
        vec![views(&[]), ok(EMPTY_PAGE), members(&["m1"], 1)],
    );
    result.expect("a cycle that stores an empty library list");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);

    let (result, requests) = cycle(&library, Trigger::Requested, failed_list());
    let report = result.expect("a cycle whose library list failed");
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);
    assert_eq!(report.deleted, 0);
    assert!(library.item("m1").expect("query").is_some());
}

#[test]
fn a_library_removed_by_a_failed_cycle_is_swept_by_the_next() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1"]),
            (MORE_MOVIES, "movies", &["m2"]),
        ],
    );
    let remaining = || views(&[(MOVIES, "movies")]);

    let (result, _) = cycle(
        &library,
        Trigger::Scheduled,
        vec![remaining(), ok(NOT_JSON)],
    );
    assert!(matches!(result, Err(ApiError::Decode(_))), "{result:?}");

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            remaining(),
            ok(EMPTY_PAGE),
            members(&["m1"], 1),
            members(&["m1"], 1),
        ],
    );
    let report = result.expect("the next cycle");
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
    assert_eq!(report.deleted, 1);
    assert!(library.item("m2").expect("query").is_none());

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![remaining(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

const MIXED: &str = "a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7";
const COUNTED_EMPTY: &str = r#"{"Items":[],"TotalRecordCount":1}"#;

fn cached(library: &Library, ids: &[&str]) -> Vec<String> {
    ids.iter()
        .filter(|id| library.item(id).expect("query").is_some())
        .map(|id| (*id).to_string())
        .collect()
}

fn watch_state_rows(library: &Library, ids: &[&str]) -> i64 {
    library
        .db
        .with_connection(|connection| {
            ids.iter().try_fold(0, |rows, id| {
                connection
                    .query_row(
                        "SELECT count(*) FROM user_data WHERE jellyfin_id = ?1",
                        [id],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|count| rows + count)
            })
        })
        .expect("user data")
}

fn one_listed_title() -> Arc<Library> {
    let library = settled_library(&[("m1", "Movie")]);
    sync_libraries(&library, &[(MOVIES, "movies", &["m1"])]);
    library
}

fn requested_sweep(library: &Arc<Library>, sweep: Vec<Reply>) -> Result<SyncReport, ApiError> {
    let mut replies = vec![views(&[(MOVIES, "movies")]), ok(EMPTY_PAGE)];
    replies.extend(sweep);
    replies.push(members(&[], 0));
    cycle(library, Trigger::Requested, replies).0
}

const A_FEW_MINUTES: Duration = Duration::from_secs(6 * 60);
const ONE_CYCLE: Duration = Duration::from_secs(11 * 60);
const HOURS: Duration = Duration::from_secs(3 * 60 * 60);

fn pass_time(library: &Library, elapsed: Duration) {
    shift_empty_observation(library, -(elapsed.as_secs() as i64));
}

fn turn_clock_back(library: &Library, by: Duration) {
    shift_empty_observation(library, by.as_secs() as i64);
}

fn shift_empty_observation(library: &Library, seconds: i64) {
    let observed = library
        .meta(META_EMPTY_SWEEP_AT)
        .expect("meta")
        .and_then(|at| at.parse::<i64>().ok());
    if let Some(at) = observed {
        set_meta(library, META_EMPTY_SWEEP_AT, &(at + seconds).to_string());
    }
}

fn empty_sweep(library: &Arc<Library>) -> usize {
    requested_sweep(library, vec![ok(EMPTY_PAGE)])
        .expect("an empty answer")
        .deleted
}

fn full_identity_page() -> String {
    let items = (0..IDENTITY_PAGE_SIZE)
        .map(|index| json!({ "Id": format!("t{index:04}") }))
        .collect::<Vec<_>>();
    json!({ "Items": items, "TotalRecordCount": IDENTITY_PAGE_SIZE * 2 }).to_string()
}

#[test]
fn a_title_a_mixed_library_still_shows_stays_when_its_catalog_library_goes() {
    let library = settled_library(&[("m1", "Movie"), ("m2", "Movie")]);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1"]),
            (MORE_MOVIES, "movies", &["m2"]),
            (MIXED, "folders", &[]),
        ],
    );

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            views(&[(MOVIES, "movies"), (MIXED, "folders")]),
            ok(EMPTY_PAGE),
            members(&["m1", "m2"], 2),
            members(&["m1"], 1),
        ],
    );

    let report = result.expect("the cycle that notices the removal");
    assert_eq!(report.deleted, 0);
    assert_eq!(cached(&library, &["m1", "m2"]), ["m1", "m2"]);
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk movies"]
    );
}

#[test]
fn a_partial_library_list_keeps_titles_the_server_still_returns() {
    let library = settled_library(&[("m1", "Movie"), ("s1", "Series")]);
    sync_libraries(
        &library,
        &[(MOVIES, "movies", &["m1"]), (SHOWS, "tvshows", &["s1"])],
    );

    let (result, _) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            views(&[(MOVIES, "movies")]),
            ok(EMPTY_PAGE),
            members(&["m1", "s1"], 2),
            members(&["m1"], 1),
        ],
    );

    let report = result.expect("a cycle whose library list omits a library");
    assert_eq!(report.deleted, 0);
    assert_eq!(cached(&library, &["m1", "s1"]), ["m1", "s1"]);
}

#[test]
fn an_empty_answer_counting_zero_deletes_once_a_sweep_a_cycle_later_sees_the_same() {
    let library = Arc::new(Library::open_in_memory().expect("library"));
    seed(
        &library,
        &[
            json!({ "Id": "m1", "Name": "Movie", "Type": "Movie" }),
            json!({ "Id": "s1", "Name": "Show", "Type": "Series",
                "UserData": { "IsFavorite": true } }),
            json!({ "Id": "se1", "Name": "Season 1", "Type": "Season",
                "ParentId": "s1", "SeriesId": "s1" }),
            json!({ "Id": "e1", "Name": "Pilot", "Type": "Episode",
                "ParentId": "se1", "SeriesId": "s1", "SeasonId": "se1",
                "UserData": { "Played": true, "PlayCount": 1 } }),
        ],
    );
    settled_catalog(&library);
    sync_libraries(
        &library,
        &[
            (MOVIES, "movies", &["m1"]),
            (SHOWS, "tvshows", &["s1"]),
            (MORE_MOVIES, "movies", &[]),
        ],
    );
    let titles = ["m1", "s1", "se1", "e1"];
    let empty_library_left = || views(&[(MORE_MOVIES, "movies")]);
    assert_eq!(watch_state_rows(&library, &titles), 2);

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![empty_library_left(), ok(EMPTY_PAGE), ok(EMPTY_PAGE)],
    );
    let report = result.expect("the first empty answer");
    assert_eq!(report.deleted, 0);
    assert_eq!(cached(&library, &titles), titles);
    assert_eq!(steps(&requests), ["views", "incremental", "sweep"]);

    pass_time(&library, ONE_CYCLE);
    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![
            empty_library_left(),
            ok(EMPTY_PAGE),
            ok(EMPTY_PAGE),
            members(&[], 0),
        ],
    );
    let report = result.expect("the confirming empty answer");
    assert_eq!(report.deleted, 4);
    assert!(cached(&library, &titles).is_empty());
    assert_eq!(watch_state_rows(&library, &titles), 0);
    assert_eq!(
        report.changes,
        LibraryChangeBatch {
            item_ids: ["e1", "m1", "s1", "se1"].map(String::from).to_vec(),
            context_ids: ["s1", "se1"].map(String::from).to_vec(),
        }
    );
    assert_eq!(
        steps(&requests),
        ["views", "incremental", "sweep", "walk more movies"]
    );

    let (result, requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![empty_library_left(), ok(EMPTY_PAGE)],
    );
    result.expect("a cycle with nothing owed");
    assert_eq!(steps(&requests), ["views", "incremental"]);
}

#[test]
fn an_empty_answer_that_counts_titles_or_none_deletes_nothing() {
    let library = one_listed_title();

    for answer in [EMPTY_PAGE, COUNTED_EMPTY, EMPTY_PAGE, UNCOUNTED, EMPTY_PAGE] {
        let report = requested_sweep(&library, vec![ok(answer)]).expect(answer);
        assert_eq!(report.deleted, 0, "{answer}");
        pass_time(&library, ONE_CYCLE);
    }
    let report = requested_sweep(&library, vec![ok(COUNTED_EMPTY)]).expect("counted");
    assert_eq!(report.deleted, 0);
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
}

#[test]
fn a_non_empty_answer_between_two_empty_ones_resets_the_confirmation() {
    let library = one_listed_title();

    for sweep in [ok(EMPTY_PAGE), members(&["m1"], 1), ok(EMPTY_PAGE)] {
        let report = requested_sweep(&library, vec![sweep]).expect("cycle");
        assert_eq!(report.deleted, 0);
        pass_time(&library, ONE_CYCLE);
    }
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
}

#[test]
fn a_truncated_walk_neither_deletes_nor_confirms_an_empty_answer() {
    let library = one_listed_title();
    let report = requested_sweep(&library, vec![ok(EMPTY_PAGE)]).expect("first empty answer");
    assert_eq!(report.deleted, 0);

    pass_time(&library, ONE_CYCLE);
    let page = full_identity_page();
    let server = serve(
        [views(&[(MOVIES, "movies")]), ok(EMPTY_PAGE)]
            .into_iter()
            .chain((0..MAX_IDENTITY_PAGES).map(|_| ok(&page)))
            .chain([members(&[], 0)])
            .collect(),
    );
    let session = authenticated_session(&library, &server.url);
    let report =
        super::super::run_cycle(&library, &session, Trigger::Requested).expect("truncated");
    let requests = server.requests();
    assert_eq!(
        steps(&requests)
            .iter()
            .filter(|step| **step == "sweep")
            .count(),
        MAX_IDENTITY_PAGES
    );
    assert_eq!(report.deleted, 0);
    assert_eq!(cached(&library, &["m1"]), ["m1"]);

    pass_time(&library, ONE_CYCLE);
    let report = requested_sweep(&library, vec![ok(EMPTY_PAGE)]).expect("empty again");
    assert_eq!(report.deleted, 0);
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
}

#[test]
fn a_failed_sweep_resets_the_confirmation() {
    let library = one_listed_title();
    let report = requested_sweep(&library, vec![ok(EMPTY_PAGE)]).expect("first empty answer");
    assert_eq!(report.deleted, 0);

    pass_time(&library, A_FEW_MINUTES);
    let failed = requested_sweep(
        &library,
        vec![
            ok(&full_identity_page()),
            Reply::Drop,
            Reply::Drop,
            Reply::Drop,
        ],
    );
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(cached(&library, &["m1"]), ["m1"]);

    pass_time(&library, A_FEW_MINUTES);
    let report = requested_sweep(&library, vec![ok(EMPTY_PAGE)]).expect("empty again");
    assert_eq!(report.deleted, 0);
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
}

#[test]
fn an_account_switch_does_not_carry_an_empty_answer_over() {
    let library = one_listed_title();
    let report = requested_sweep(&library, vec![ok(EMPTY_PAGE)]).expect("first empty answer");
    assert_eq!(report.deleted, 0);

    library.clear_session(true).expect("switch account");
    seed(
        &library,
        &[json!({ "Id": "m1", "Name": "M1", "Type": "Movie" })],
    );
    settled_catalog(&library);
    sync_libraries(&library, &[(MOVIES, "movies", &["m1"])]);

    pass_time(&library, ONE_CYCLE);
    let report = requested_sweep(&library, vec![ok(EMPTY_PAGE)]).expect("new account");
    assert_eq!(report.deleted, 0);
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
}

#[test]
fn an_empty_answer_seconds_later_does_not_confirm_and_keeps_the_first_observation() {
    let library = one_listed_title();

    assert_eq!(empty_sweep(&library), 0);
    assert_eq!(empty_sweep(&library), 0, "seconds later");
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
    pass_time(&library, A_FEW_MINUTES);
    assert_eq!(empty_sweep(&library), 0, "a few minutes later");
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
    pass_time(&library, A_FEW_MINUTES);
    assert_eq!(empty_sweep(&library), 1, "a cycle after the first answer");
    assert!(cached(&library, &["m1"]).is_empty());
}

#[test]
fn an_observation_older_than_its_window_is_replaced_instead_of_confirmed() {
    let library = one_listed_title();

    assert_eq!(empty_sweep(&library), 0);
    pass_time(&library, HOURS);
    assert_eq!(empty_sweep(&library), 0, "hours later");
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
    pass_time(&library, ONE_CYCLE);
    assert_eq!(empty_sweep(&library), 1, "a cycle after the fresh answer");
    assert!(cached(&library, &["m1"]).is_empty());
}

#[test]
fn an_observation_kept_across_a_sign_out_does_not_confirm_hours_later() {
    let library = one_listed_title();
    assert_eq!(empty_sweep(&library), 0);

    library.clear_session(false).expect("sign out");
    pass_time(&library, HOURS);

    assert_eq!(empty_sweep(&library), 0, "signed in again the next morning");
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
}

#[test]
fn an_observation_stamped_ahead_of_the_clock_is_replaced_instead_of_confirmed() {
    let library = one_listed_title();
    assert_eq!(empty_sweep(&library), 0);

    turn_clock_back(&library, HOURS);
    assert_eq!(empty_sweep(&library), 0, "after the clock moved back");
    assert_eq!(cached(&library, &["m1"]), ["m1"]);
    pass_time(&library, ONE_CYCLE);
    assert_eq!(empty_sweep(&library), 1, "a cycle after the fresh answer");
}

#[test]
fn a_storage_error_while_forgetting_an_observation_fails_the_cycle() {
    let library = one_listed_title();
    assert_eq!(empty_sweep(&library), 0);
    library
        .db
        .with_connection(|connection| {
            connection.execute_batch(&format!(
                "CREATE TRIGGER refuse_forgetting BEFORE UPDATE ON meta
                 WHEN NEW.key = '{META_EMPTY_SWEEP_AT}' AND NEW.value = ''
                 BEGIN SELECT RAISE(ABORT, 'storage refused'); END;"
            ))
        })
        .expect("trigger");

    let failed = requested_sweep(
        &library,
        vec![
            ok(&full_identity_page()),
            Reply::Drop,
            Reply::Drop,
            Reply::Drop,
        ],
    );

    assert!(matches!(failed, Err(ApiError::Storage(_))), "{failed:?}");
}
