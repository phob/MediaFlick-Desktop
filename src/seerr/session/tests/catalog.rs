use super::support::*;
use super::*;

// ------------------------------------------------------------------ reads

const SEARCH: &str = r#"{"page":1,"totalPages":1,"totalResults":3,"results":[
    {"id":603,"mediaType":"movie","title":"The Matrix","releaseDate":"1999-03-30",
     "posterPath":"/matrix.jpg","mediaInfo":{"tmdbId":603,"status":5,"status4k":1}},
    {"id":603,"mediaType":"tv","name":"Not The Matrix","firstAirDate":"2010-01-01"},
    {"id":6384,"mediaType":"person","name":"Keanu Reeves"}]}"#;

const PERSON_CREDITS: &str = r#"{"id":6384,"cast":[
    {"id":603,"mediaType":"movie","title":"The Matrix","character":"Neo","adult":false},
    {"id":603,"mediaType":"movie","title":"The Matrix","character":"Thomas Anderson",
     "mediaInfo":{"status":5,"status4k":1}},
    {"id":603,"mediaType":"tv","name":"A Different Namespace"},
    {"id":7,"mediaType":"movie","title":"Special Thanks","character":" thanks "},
    {"id":8,"mediaType":"movie","title":"Adult","adult":true}],
    "crew":[{"id":9,"mediaType":"movie","title":"Directed"}]}"#;

pub(super) fn seed_library(library: &Library) {
    let dto = |json: &str| serde_json::from_str(json).expect("dto");
    library
        .upsert_page(&[
            dto(r#"{"Id":"m1","Name":"The Matrix","Type":"Movie",
                    "ProviderIds":{"Tmdb":"603"}}"#),
            dto(r#"{"Id":"s1","Name":"Severance","Type":"Series",
                    "ProviderIds":{"Tmdb":"95396"}}"#),
        ])
        .expect("seed");
}

/// The join this whole chunk turns on: a result the library already has
/// resolves to that item, and one it does not resolves to a request.
#[test]
fn search_results_are_joined_to_the_library_by_kind_and_tmdb_id() {
    let (base_url, requests) = fake_server(vec![response("200 OK", SEARCH, &[])]);
    let (library, session) = session_linked_to(&base_url);
    seed_library(&library);

    let page = session.search("matrix", 1).expect("search");
    let results = page["results"].as_array().expect("results");
    // The person is gone: nothing to join, nothing to request.
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["mediaType"], "movie");
    assert_eq!(results[0]["title"], "The Matrix");
    assert_eq!(results[0]["year"], 1999);
    assert_eq!(results[0]["status"], "available");
    assert_eq!(results[0]["libraryItemId"], "m1");
    // Same TMDB id, other namespace: it must not inherit the movie's item.
    assert_eq!(results[1]["mediaType"], "tv");
    assert_eq!(results[1]["libraryItemId"], Value::Null);
    assert_eq!(results[1]["status"], "unknown");
    assert_eq!(page["totalResults"], 3);

    let requests = requests.lock().expect("lock");
    assert!(requests[0].starts_with("GET /api/v1/search?query=matrix&page=1"));
    assert!(requests[0].contains("cookie: connect.sid=abc"));
    drop(requests);
}

#[test]
fn exact_person_discovery_keeps_cast_deduplicated_and_joins_availability() {
    let (base_url, requests) = fake_server(vec![response("200 OK", PERSON_CREDITS, &[])]);
    let (library, session) = session_linked_to(&base_url);
    seed_library(&library);

    let page = session.person_credits(6384).expect("person credits");
    let results = page["results"].as_array().expect("results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["mediaType"], "movie");
    assert_eq!(results[0]["libraryItemId"], "m1");
    // Request/availability state came from the duplicate character row.
    assert_eq!(results[0]["status"], "available");
    assert_eq!(results[1]["mediaType"], "tv");
    assert_eq!(results[1]["libraryItemId"], Value::Null);
    assert_eq!(page["totalResults"], 2);

    assert!(
        requests.lock().expect("lock")[0]
            .starts_with("GET /api/v1/person/6384/combined_credits?language=en")
    );
}

#[test]
fn an_empty_search_term_costs_no_round_trip() {
    let (base_url, requests) = fake_server(Vec::new());
    let (_library, session) = session_linked_to(&base_url);

    let page = session.search("   ", 1).expect("search");
    assert_eq!(page["results"].as_array().map(Vec::len), Some(0));
    assert!(requests.lock().expect("lock").is_empty());
}

#[test]
fn discover_rows_are_named_rather_than_addressed() {
    let (base_url, requests) = fake_server(vec![response("200 OK", SEARCH, &[])]);
    let (_library, session) = session_linked_to(&base_url);
    let options = DiscoverOptions::from_values(None, None, None, None, Some("movie"), Some("week"))
        .expect("options");

    session
        .discover(super::DiscoverKind::Trending, 2, &options)
        .expect("discover");
    assert!(
        requests.lock().expect("lock")[0]
            .starts_with("GET /api/v1/discover/trending?page=2&mediaType=movie&timeWindow=week")
    );
    assert_eq!(
        super::DiscoverKind::from_id("movies"),
        Some(DiscoverKind::Movies)
    );
    assert_eq!(
        super::DiscoverKind::from_id("upcoming-tv").map(DiscoverKind::path),
        Some("discover/tv/upcoming")
    );
    assert_eq!(super::DiscoverKind::from_id("../settings"), None);
}

#[test]
fn discover_filters_are_allowlisted_and_shaped_for_each_media_kind() {
    let options = DiscoverOptions::from_values(
        Some("18"),
        Some("rating"),
        Some("7"),
        Some("1990"),
        None,
        None,
    )
    .expect("movie options");
    let today = UtcDate {
        year: 2026,
        month: 8,
        day: 1,
    };
    let movie = options.query_pairs_for(DiscoverKind::Movies, 4, false, today);
    assert_eq!(
        movie,
        vec![
            ("page", "4".to_string()),
            ("genre", "18".to_string()),
            ("primaryReleaseDateGte", "1990-01-01".to_string()),
            ("primaryReleaseDateLte", "1999-12-31".to_string()),
            ("sortBy", "vote_average.desc".to_string()),
            ("voteCountGte", "50".to_string()),
            ("voteAverageGte", "7".to_string()),
        ]
    );
    assert!(
        options
            .companion_query_pairs(DiscoverKind::Movies, 4)
            .contains(&("releaseDecade", "1990".to_string()))
    );

    let tv_options =
        DiscoverOptions::from_values(None, Some("newest"), None, Some("2020"), None, None)
            .expect("tv options");
    let tv = tv_options.query_pairs_for(DiscoverKind::Tv, 1, false, today);
    assert!(tv.contains(&("sortBy", "first_air_date.desc".to_string())));
    assert!(tv.contains(&("firstAirDateGte", "2020-01-01".to_string())));
    assert!(tv.contains(&("firstAirDateLte", "2026-08-01".to_string())));
    assert!(
        DiscoverOptions::from_values(Some("../settings"), None, None, None, None, None).is_err()
    );
    assert!(DiscoverOptions::from_values(None, Some("random"), None, None, None, None).is_err());
    assert!(DiscoverOptions::from_values(None, None, None, Some("1995"), None, None).is_err());
    assert!(DiscoverOptions::from_values(None, None, None, Some("1890"), None, None).is_err());
    assert!(DiscoverOptions::from_values(None, None, None, Some("9990"), None, None).is_err());
    let earliest =
        DiscoverOptions::from_values(None, Some("popular"), None, Some("1900"), None, None)
            .expect("earliest decade");
    let earliest_movie = earliest.query_pairs_for(DiscoverKind::Movies, 1, false, today);
    assert!(earliest_movie.contains(&("primaryReleaseDateGte", "1900-01-01".to_string())));
    assert!(earliest_movie.contains(&("primaryReleaseDateLte", "1909-12-31".to_string())));
    assert_eq!(
        UtcDate::from_unix_days(0),
        UtcDate {
            year: 1970,
            month: 1,
            day: 1,
        }
    );
}

#[test]
fn a_series_detail_offers_the_seasons_that_can_be_requested() {
    let (base_url, requests) = fake_server(vec![response(
        "200 OK",
        r#"{"id":95396,"name":"Severance","firstAirDate":"2022-02-18",
            "episodeRunTime":[45],"genres":[{"id":18,"name":"Drama"}],
            "externalIds":{"imdbId":"tt11280740","tvdbId":371980},
            "seasons":[{"id":1,"seasonNumber":0,"name":"Specials","episodeCount":2},
                       {"id":2,"seasonNumber":1,"name":"Season 1","episodeCount":9},
                       {"id":3,"seasonNumber":2,"name":"Season 2","episodeCount":10}],
            "mediaInfo":{"tmdbId":95396,"status":4,"status4k":1,
                         "seasons":[{"seasonNumber":1,"status":5,"status4k":1}]}}"#,
        &[],
    )]);
    let (library, session) = session_linked_to(&base_url);
    seed_library(&library);

    let detail = session.media_detail("tv", 95396).expect("detail");
    assert_eq!(detail["title"], "Severance");
    assert_eq!(detail["status"], "partial");
    assert_eq!(detail["runtimeMinutes"], 45);
    assert_eq!(detail["genres"][0], "Drama");
    assert_eq!(detail["libraryItemId"], "s1");
    assert_eq!(detail["externalIds"]["imdb"], "tt11280740");
    assert_eq!(detail["externalIds"]["tvdb"], 371_980);

    let seasons = detail["seasons"].as_array().expect("seasons");
    // Specials are left out, exactly as Seerr's own request modal does.
    assert_eq!(seasons.len(), 2);
    assert_eq!(seasons[0]["seasonNumber"], 1);
    assert_eq!(seasons[0]["status"], "available");
    assert_eq!(seasons[1]["seasonNumber"], 2);
    assert_eq!(seasons[1]["status"], "unknown");

    assert!(requests.lock().expect("lock")[0].starts_with("GET /api/v1/tv/95396 HTTP/1.1"));
}

#[test]
fn a_movie_detail_keeps_rich_metadata_and_safe_trailer_release_data() {
    let (base_url, _) = fake_server(vec![response(
        "200 OK",
        r#"{"id":603,"title":"The Matrix","originalTitle":"The Matrix",
            "releaseDate":"1999-03-30","status":"Released","runtime":136,
            "overview":"A hacker discovers the truth.","tagline":"Welcome to the Real World.",
            "imdbId":"not-an-imdb-id",
            "externalIds":{"imdbId":"tt0133093","tvdbId":-1},
            "voteAverage":8.2,"voteCount":26000,"originalLanguage":"en",
            "budget":63000000,"revenue":467200000,
            "genres":[{"id":28,"name":"Action"}],
            "productionCompanies":[{"id":79,"name":"Village Roadshow Pictures"}],
            "productionCountries":[{"iso_3166_1":"US","name":"United States"}],
            "spokenLanguages":[{"iso_639_1":"en","name":"English","englishName":"English"}],
            "credits":{"cast":[{"id":6384,"name":"Keanu Reeves","character":"Neo",
                                "profilePath":"/keanu.jpg"}],
                       "crew":[{"id":1,"name":"Lana Wachowski","job":"Director",
                                "department":"Directing"},
                               {"id":2,"name":"Lilly Wachowski","job":"Screenplay",
                                "department":"Writing"}]},
            "relatedVideos":[{"site":"YouTube","type":"Trailer","key":"abcdefghijk",
                              "name":"Official Trailer","size":1080},
                             {"site":"YouTube","type":"Trailer","key":"not/a/key",
                              "name":"Unsafe","size":2160}],
            "releases":{"results":[{"iso_3166_1":"US","release_dates":[
                {"type":3,"release_date":"1999-03-31T00:00:00.000Z","certification":"R"}]}]},
            "mediaInfo":{"tmdbId":603,"status":5,"status4k":1}}"#,
        &[],
    )]);
    let (library, session) = session_linked_to(&base_url);
    seed_library(&library);

    let detail = session.media_detail("movie", 603).expect("detail");
    assert_eq!(detail["overview"], "A hacker discovers the truth.");
    assert_eq!(detail["productionStatus"], "Released");
    assert_eq!(detail["voteAverage"], 8.2);
    assert_eq!(detail["voteCount"], 26_000);
    assert_eq!(detail["directors"], json!(["Lana Wachowski"]));
    assert_eq!(detail["writers"], json!(["Lilly Wachowski"]));
    assert_eq!(detail["cast"][0]["character"], "Neo");
    assert_eq!(detail["externalIds"]["imdb"], "tt0133093");
    assert_eq!(detail["externalIds"]["tvdb"], Value::Null);
    assert_eq!(detail["trailer"]["key"], "abcdefghijk");
    assert_eq!(detail["releaseDates"][0]["type"], "cinema");
    assert_eq!(detail["releaseDates"][0]["certification"], "R");
}

/// A `person` id in the media route would address an unrelated title, so it
/// is refused before a request is sent.
#[test]
fn a_detail_for_a_kind_that_is_not_requestable_is_refused_locally() {
    let (base_url, requests) = fake_server(Vec::new());
    let (_library, session) = session_linked_to(&base_url);

    assert!(matches!(
        session.media_detail("person", 6384),
        Err(SeerrError::Unusable(_))
    ));
    assert!(matches!(
        session.media_detail("movie", 0),
        Err(SeerrError::Unusable(_))
    ));
    assert!(requests.lock().expect("lock").is_empty());
}

// ----------------------------------------------------------------- writes
