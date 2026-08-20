use super::catalog::seed_library;
use super::support::*;
use super::*;

#[test]
fn requesting_a_movie_sends_one_unretried_write() {
    let (base_url, requests) = fake_server(vec![response(
        "201 Created",
        r#"{"id":12,"status":1,"type":"movie","is4k":false,"createdAt":"2026-07-27T10:00:00Z",
            "media":{"tmdbId":603,"mediaType":"movie","status":2,"status4k":1}}"#,
        &[],
    )]);
    let (library, session) = session_linked_to(&base_url);
    seed_library(&library);

    let created = session
        .create_request("movie", 603, None, false, None)
        .expect("request");
    assert_eq!(created["id"], 12);
    assert_eq!(created["status"], "pending");
    assert_eq!(created["mediaStatus"], "pending");
    assert_eq!(created["tmdbId"], 603);
    assert_eq!(created["libraryItemId"], "m1");

    let requests = requests.lock().expect("lock");
    assert_eq!(requests.len(), 1, "a write must never be retried");
    assert!(requests[0].starts_with("POST /api/v1/request HTTP/1.1"));
    let body = compact(&requests[0]);
    drop(requests);
    assert!(body.contains(r#""mediaType":"movie""#));
    assert!(body.contains(r#""mediaId":603"#));
    // Movies have no seasons; sending an empty list would be rejected.
    assert!(!body.contains("seasons"));
}

#[test]
fn requesting_named_seasons_asks_for_exactly_those() {
    let (base_url, requests) = fake_server(vec![response(
        "201 Created",
        r#"{"id":13,"status":1,"type":"tv","media":{"tmdbId":95396,"mediaType":"tv","status":2},
            "seasons":[{"seasonNumber":2,"status":1}]}"#,
        &[],
    )]);
    let (_library, session) = session_linked_to(&base_url);

    let created = session
        .create_request("tv", 95396, Some(vec![2]), false, None)
        .expect("request");
    assert_eq!(created["seasons"], json!([2]));
    assert!(compact(&requests.lock().expect("lock")[0]).contains(r#""seasons":[2]"#));
}

#[test]
fn advanced_request_options_are_scoped_to_the_matching_download_service() {
    let (base_url, requests) = fake_server(vec![
        response(
            "200 OK",
            r#"{"id":7,"displayName":"pho","jellyfinUserId":"uid","permissions":8224}"#,
            &[],
        ),
        response(
            "200 OK",
            r#"[{"id":0,"name":"Movies","is4k":false,"isDefault":true,"activeProfileId":2},
                {"id":1,"name":"Movies 4K","is4k":true,"isDefault":false,"activeProfileId":3}]"#,
            &[],
        ),
        response(
            "200 OK",
            r#"{"profiles":[{"id":2,"name":"HD-1080p"},{"id":1,"name":"Any"}]}"#,
            &[],
        ),
    ]);
    let (_library, session) = session_linked_to(&base_url);

    let options = session.request_options("movie", false).expect("options");
    let destination = &options["destinations"][0];
    assert_eq!(destination["id"], 0);
    assert_eq!(destination["name"], "Movies");
    assert_eq!(destination["profiles"][0]["name"], "Any");
    assert_eq!(destination["profiles"][1]["isDefault"], true);

    let requests = requests.lock().expect("lock");
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /api/v1/auth/me HTTP/1.1"));
    assert!(requests[1].starts_with("GET /api/v1/service/radarr HTTP/1.1"));
    assert!(requests[2].starts_with("GET /api/v1/service/radarr/0 HTTP/1.1"));
    drop(requests);
}

#[test]
fn a_selected_profile_is_permission_checked_and_sent_with_one_write() {
    let (base_url, requests) = fake_server(vec![
        response(
            "200 OK",
            r#"{"id":7,"displayName":"pho","jellyfinUserId":"uid","permissions":8224}"#,
            &[],
        ),
        response(
            "201 Created",
            r#"{"id":15,"status":1,"type":"movie",
                "media":{"tmdbId":603,"mediaType":"movie","status":2}}"#,
            &[],
        ),
    ]);
    let (_library, session) = session_linked_to(&base_url);

    session
        .create_request(
            "movie",
            603,
            None,
            false,
            Some(RequestProfileSelection {
                server_id: 0,
                profile_id: 2,
            }),
        )
        .expect("request");

    let requests = requests.lock().expect("lock");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("POST "))
            .count(),
        1,
        "a write must never be retried"
    );
    let body = compact(&requests[1]);
    drop(requests);
    assert!(body.contains(r#""serverId":0"#));
    assert!(body.contains(r#""profileId":2"#));
}

/// "Request this show" with no season named is Seerr's own `all`, which it
/// expands to whatever it does not already have.
#[test]
fn requesting_a_series_without_naming_seasons_asks_for_all_of_them() {
    let (base_url, requests) = fake_server(vec![response(
        "201 Created",
        r#"{"id":14,"status":1,"type":"tv","media":{"tmdbId":95396,"mediaType":"tv"}}"#,
        &[],
    )]);
    let (_library, session) = session_linked_to(&base_url);

    session
        .create_request("tv", 95396, None, false, None)
        .expect("request");
    assert!(compact(&requests.lock().expect("lock")[0]).contains(r#""seasons":"all""#));
}

#[test]
fn requests_are_scoped_to_the_signed_in_seerr_user() {
    let (base_url, requests) = fake_server(vec![response(
        "200 OK",
        r#"{"pageInfo":{"pages":1,"pageSize":20,"results":1,"page":1},
            "results":[{"id":12,"status":2,"type":"movie","is4k":false,
                        "media":{"tmdbId":603,"mediaType":"movie","status":5,"status4k":1}}]}"#,
        &[],
    )]);
    let (library, session) = session_linked_to(&base_url);
    seed_library(&library);

    let page = session.requests(20, 0, "all").expect("requests");
    assert_eq!(page["totalResults"], 1);
    let first = &page["results"][0];
    assert_eq!(first["status"], "approved");
    assert_eq!(first["mediaStatus"], "available");
    // Available and already in the library: the card links to the item.
    assert_eq!(first["libraryItemId"], "m1");

    let requests = requests.lock().expect("lock");
    assert!(requests[0].starts_with("GET /api/v1/request?"));
    assert!(requests[0].contains("requestedBy=7"), "{}", requests[0]);
    assert!(requests[0].contains("take=20"));
    drop(requests);
}

/// A 4K request reports the 4K availability, not the ordinary one.
#[test]
fn a_four_k_request_reports_the_four_k_status() {
    let (base_url, _) = fake_server(vec![response(
        "200 OK",
        r#"{"pageInfo":{"pages":1,"page":1,"results":1},
            "results":[{"id":15,"status":1,"type":"movie","is4k":true,
                        "media":{"tmdbId":603,"mediaType":"movie","status":5,"status4k":2}}]}"#,
        &[],
    )]);
    let (_library, session) = session_linked_to(&base_url);

    let page = session
        .requests(20, 0, "nonsense filter")
        .expect("requests");
    assert_eq!(page["results"][0]["mediaStatus"], "pending");
}

#[test]
fn cancelling_a_request_sends_a_delete_with_the_csrf_header() {
    let (base_url, requests) = fake_server(vec![response("204 No Content", "", &[])]);
    let library = library();
    signed_in(&library, "uid");
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some(base_url),
            cookies: Some(r#"{"XSRF-TOKEN":"token123","connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            ..SeerrConfig::default()
        })
        .expect("seerr config");
    let session = SeerrSession::restore(library);

    assert_eq!(
        session.cancel_request(12).expect("cancel")["cancelled"],
        true
    );
    let requests = requests.lock().expect("lock");
    assert_eq!(requests.len(), 1, "a cancellation must never be retried");
    assert!(requests[0].starts_with("DELETE /api/v1/request/12 HTTP/1.1"));
    assert!(requests[0].contains("x-xsrf-token: token123"));
    drop(requests);
}

/// Seerr answers 401 — not 403 — to a valid session that may not cancel
/// somebody else's request. Taking that at face value would sign the user
/// out for pressing a button they were never allowed to press.
#[test]
fn a_refused_write_is_a_permission_error_not_a_lapsed_session() {
    let (base_url, requests) = fake_server(vec![
        response("401 Unauthorized", r#"{"message":"Unauthorized"}"#, &[]),
        // `/auth/me` answers, so the session itself is fine.
        response("200 OK", ME, &[]),
    ]);
    let (library, session) = session_linked_to(&base_url);

    let error = session.cancel_request(12).expect_err("refused");
    assert_eq!(error, SeerrError::PermissionDenied);

    let requests = requests.lock().expect("lock");
    assert!(requests[1].starts_with("GET /api/v1/auth/me HTTP/1.1"));
    drop(requests);
    // The link survives: nothing about this was a session expiry.
    assert!(!session.read().expired);
    assert!(library.seerr_config().cookies.is_some());
    assert!(is_linked(&session));
}

#[test]
fn a_lapsed_session_is_confirmed_before_the_re_link_prompt_appears() {
    let (base_url, _) = fake_server(vec![
        response("401 Unauthorized", r#"{"message":"Unauthorized"}"#, &[]),
        response("401 Unauthorized", r#"{"message":"Unauthorized"}"#, &[]),
    ]);
    let (_library, session) = session_linked_to(&base_url);

    let error = session.cancel_request(12).expect_err("expired");
    assert_eq!(error, SeerrError::Unauthorized);
    assert!(session.read().expired);
    // Every later acquisition refuses until the user re-links.
    assert!(matches!(session.client(), Err(SeerrError::Unauthorized)));
}

#[test]
fn reads_need_a_linked_instance() {
    let session = SeerrSession::restore(library());
    assert!(matches!(
        session.search("matrix", 1),
        Err(SeerrError::NotConfigured)
    ));
    assert!(matches!(
        session.requests(20, 0, "all"),
        Err(SeerrError::NotConfigured)
    ));
    assert!(matches!(
        session.cancel_request(0),
        Err(SeerrError::Unusable(_))
    ));
}

/// Guards against the credentials row disappearing under a live session.
#[test]
fn a_link_with_no_jellyfin_binding_is_left_alone() {
    let library = library();
    signed_in(&library, "uid");
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some("https://seerr.test".to_string()),
            cookies: Some(r#"{"XSRF-TOKEN":"t"}"#.to_string()),
            ..SeerrConfig::default()
        })
        .expect("seerr config");

    let session = SeerrSession::restore(library.clone());
    session.revalidate();
    assert!(library.seerr_config().cookies.is_some());
}
