use super::support::*;
use super::*;

#[test]
fn linking_with_a_password_stores_the_session_and_the_account_it_belongs_to() {
    let (base_url, requests) = fake_server(vec![
        response("200 OK", INITIALIZED, &["XSRF-TOKEN=token123; Path=/"]),
        response("200 OK", ME, &[SESSION]),
        response("200 OK", ME, &[]),
        response("200 OK", ME, &[]),
        response("200 OK", QUOTA, &[]),
    ]);
    let library = library();
    signed_in(&library, "uid");
    configured(&library, &base_url);
    let session = SeerrSession::restore(library.clone());

    let result = session.link_with_password("pho", "hunter2").expect("link");
    assert_eq!(result["method"], "password");
    assert_eq!(result["linked"], true);
    assert_eq!(result["status"]["linked"], true);
    assert_eq!(result["status"]["user"]["name"], "pho");
    assert_eq!(result["status"]["capabilities"]["movie"]["request"], true);

    let requests = requests.lock().expect("lock");
    // A GET precedes the write, so a rotated CSRF pair is in hand before
    // the login needs it — and is echoed as the header csurf looks for.
    assert!(requests[0].starts_with("GET /api/v1/settings/public HTTP/1.1"));
    assert!(requests[1].starts_with("POST /api/v1/auth/jellyfin HTTP/1.1"));
    assert!(requests[1].contains("x-xsrf-token: token123"));
    assert!(requests[2].starts_with("GET /api/v1/auth/me HTTP/1.1"));
    assert!(requests[2].contains("cookie: XSRF-TOKEN=token123; connect.sid=s%3Aabc.def"));
    drop(requests);

    let stored = library.seerr_config();
    assert_eq!(stored.user_id, Some(7));
    assert_eq!(stored.user_name.as_deref(), Some("pho"));
    assert_eq!(stored.jellyfin_user_id.as_deref(), Some("uid"));
    assert_eq!(stored.jellyfin_server_id.as_deref(), Some("srv"));
    assert!(stored.partial_requests_enabled);
    assert!(
        stored
            .cookies
            .as_deref()
            .is_some_and(|cookies| cookies.contains("connect.sid"))
    );
    assert!(is_linked(&session));
}

/// The guard that password login rests on: Seerr cannot be asked which
/// Jellyfin server it is wired to before logging in, so the account behind
/// the session it hands back is what gets checked.
#[test]
fn a_login_as_a_different_media_server_user_is_refused_and_logged_out() {
    let (base_url, requests) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("200 OK", ME, &[SESSION]),
        response(
            "200 OK",
            r#"{"id":9,"displayName":"someone","jellyfinUserId":"another-uid"}"#,
            &[],
        ),
        response("204 No Content", "", &[]),
    ]);
    let library = library();
    signed_in(&library, "uid");
    configured(&library, &base_url);
    let session = SeerrSession::restore(library.clone());

    let error = session
        .link_with_password("someone", "hunter2")
        .expect_err("refused");
    assert!(matches!(error, SeerrError::Unusable(_)));
    assert!(error.to_string().contains("different media-server user"));

    let requests = requests.lock().expect("lock");
    assert_eq!(requests.len(), 4, "the refused session was not logged out");
    assert!(requests[3].starts_with("POST /api/v1/auth/logout HTTP/1.1"));
    drop(requests);

    // Fail closed: nothing about the refused session is on disk.
    let stored = library.seerr_config();
    assert_eq!(stored.cookies, None);
    assert_eq!(stored.user_id, None);
    assert_eq!(stored.jellyfin_user_id, None);
    assert!(!is_linked(&session));
}

#[test]
fn a_login_seerr_cannot_attribute_to_an_account_is_refused() {
    let (base_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("200 OK", "{}", &[SESSION]),
        response("200 OK", r#"{"id":9,"displayName":"someone"}"#, &[]),
        response("204 No Content", "", &[]),
    ]);
    let library = library();
    signed_in(&library, "uid");
    configured(&library, &base_url);

    let error = SeerrSession::restore(library.clone())
        .link_with_password("someone", "hunter2")
        .expect_err("refused");
    assert!(error.to_string().contains("did not say which"));
    assert_eq!(library.seerr_config().cookies, None);
}

/// Jellyfin hands its GUIDs out both with and without dashes; a plain
/// comparison would read a match as the account switch this guard exists
/// to catch.
#[test]
fn the_account_guard_ignores_how_the_id_is_punctuated() {
    assert!(same_media_server_user(
        "8AB2E0F0-3B5C-4D3E-9F00-000000000001",
        "8ab2e0f03b5c4d3e9f00000000000001"
    ));
    assert!(!same_media_server_user("uid", "other-uid"));
    assert!(!same_media_server_user("", ""));
}

#[test]
fn a_user_seerr_has_never_imported_gets_its_own_message() {
    let (base_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("403 Forbidden", r#"{"message":"Access denied"}"#, &[]),
    ]);
    let library = library();
    signed_in(&library, "uid");
    configured(&library, &base_url);

    let error = SeerrSession::restore(library)
        .link_with_password("pho", "hunter2")
        .expect_err("refused");
    assert_eq!(error, SeerrError::UnknownUser);
    assert!(error.to_string().contains("administrator"));
}

/// A mistyped password must not read as a lapsed session: the user is
/// establishing one, and `Unauthorized` is what puts the UI into the
/// re-link prompt they are already in.
#[test]
fn a_rejected_password_is_not_reported_as_a_lapsed_session() {
    let (base_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("401 Unauthorized", r#"{"message":"Unauthorized"}"#, &[]),
    ]);
    let library = library();
    signed_in(&library, "uid");
    configured(&library, &base_url);

    let error = SeerrSession::restore(library)
        .link_with_password("pho", "wrong")
        .expect_err("rejected");
    assert_eq!(error, SeerrError::LoginRejected);
}

#[test]
fn linking_needs_an_instance_and_credentials_before_anything_is_sent() {
    let library = library();
    signed_in(&library, "uid");
    let session = SeerrSession::restore(library.clone());
    assert!(matches!(
        session.link_with_password("pho", "hunter2"),
        Err(SeerrError::NotConfigured)
    ));
    assert!(matches!(
        session.link_start(),
        Err(SeerrError::NotConfigured)
    ));

    configured(&library, "https://seerr.test");
    let session = SeerrSession::restore(library);
    assert!(matches!(
        session.link_with_password("  ", "hunter2"),
        Err(SeerrError::Unusable(_))
    ));
    assert!(matches!(
        session.link_poll("  "),
        Err(SeerrError::Unusable(_))
    ));
}

#[test]
fn unlinking_ends_the_session_at_the_instance_and_keeps_only_the_address() {
    let (base_url, requests) = fake_server(vec![response("204 No Content", "", &[])]);
    let library = library();
    signed_in(&library, "uid");
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some(base_url.clone()),
            cookies: Some(r#"{"XSRF-TOKEN":"token123","connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            partial_requests_enabled: true,
            ..SeerrConfig::default()
        })
        .expect("seerr config");
    let session = SeerrSession::restore(library.clone());

    let status = session.unlink();
    assert_eq!(status["linked"], false);
    assert_eq!(status["configured"], true);

    let requests = requests.lock().expect("lock");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /api/v1/auth/logout HTTP/1.1"));
    assert!(requests[0].contains("x-xsrf-token: token123"));
    drop(requests);

    let stored = library.seerr_config();
    assert_eq!(stored.base_url.as_deref(), Some(base_url.as_str()));
    assert_eq!(stored.cookies, None);
    assert_eq!(stored.user_id, None);
    assert_eq!(stored.jellyfin_user_id, None);
}

#[test]
fn quick_connect_links_without_a_password_when_both_halves_support_it() {
    let (seerr_url, seerr_requests) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("200 OK", r#"{"code":"AB12CD","secret":"s3cret"}"#, &[]),
        response("200 OK", ME, &[SESSION]),
        response("200 OK", ME, &[]),
        response("200 OK", ME, &[]),
        response("200 OK", QUOTA, &[]),
    ]);
    let (jellyfin_url, jellyfin_requests) = fake_server(vec![
        response("200 OK", "true", &[]),
        response("200 OK", "true", &[]),
    ]);
    let library = library();
    signed_in_to(&library, &jellyfin_url, "uid");
    configured(&library, &seerr_url);
    let session = SeerrSession::restore(library.clone());

    let result = session.link_start().expect("link");
    assert_eq!(result["method"], "quickconnect");
    assert_eq!(result["linked"], true);
    assert_eq!(result["status"]["user"]["name"], "pho");

    let seerr_requests = seerr_requests.lock().expect("lock");
    assert!(
        seerr_requests[1].starts_with("POST /api/v1/auth/jellyfin/quickconnect/initiate HTTP/1.1")
    );
    assert!(
        seerr_requests[2]
            .starts_with("POST /api/v1/auth/jellyfin/quickconnect/authenticate HTTP/1.1")
    );
    drop(seerr_requests);
    // The code Seerr minted is approved on our own server, by us — this is
    // the step that makes the flow password-less, and the one that proves
    // the handshake belongs to the server we are signed in to.
    let jellyfin_requests = jellyfin_requests.lock().expect("lock");
    assert!(jellyfin_requests[0].starts_with("GET /QuickConnect/Enabled HTTP/1.1"));
    assert!(jellyfin_requests[1].starts_with("POST /QuickConnect/Authorize?code=AB12CD HTTP/1.1"));
    drop(jellyfin_requests);

    assert_eq!(library.seerr_config().user_id, Some(7));
    assert!(is_linked(&session));
}

/// Quick Connect is off by default on Jellyfin, so this is the common case
/// — and it must not look like a failure.
#[test]
fn quick_connect_defers_to_the_password_path_when_the_server_has_it_off() {
    let (seerr_url, seerr_requests) = fake_server(vec![response("200 OK", INITIALIZED, &[])]);
    let (jellyfin_url, _) = fake_server(vec![response("200 OK", "false", &[])]);
    let library = library();
    signed_in_to(&library, &jellyfin_url, "uid");
    configured(&library, &seerr_url);

    let result = SeerrSession::restore(library)
        .link_start()
        .expect("no error surfaces");
    assert_eq!(result["method"], "password");
    assert_eq!(result["linked"], false);
    // No handshake was started, so none is left dangling on the instance.
    assert_eq!(seerr_requests.lock().expect("lock").len(), 1);
}

/// The Quick Connect login routes are absent from every stable release up
/// to and including v3.3.0.
#[test]
fn quick_connect_defers_to_the_password_path_when_seerr_has_no_such_route() {
    let (seerr_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("404 Not Found", r#"{"message":"Not Found"}"#, &[]),
    ]);
    let (jellyfin_url, _) = fake_server(vec![response("200 OK", "true", &[])]);
    let library = library();
    signed_in_to(&library, &jellyfin_url, "uid");
    configured(&library, &seerr_url);

    let result = SeerrSession::restore(library)
        .link_start()
        .expect("no error surfaces");
    assert_eq!(result["method"], "password");
}

/// A server that refuses to approve the code — because the handshake is not
/// its own — is the same fallback, not an error.
#[test]
fn a_handshake_our_server_will_not_approve_falls_back_to_the_password_path() {
    let (seerr_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("200 OK", r#"{"code":"AB12CD","secret":"s3cret"}"#, &[]),
    ]);
    let (jellyfin_url, _) = fake_server(vec![
        response("200 OK", "true", &[]),
        response("403 Forbidden", "{}", &[]),
    ]);
    let library = library();
    signed_in_to(&library, &jellyfin_url, "uid");
    configured(&library, &seerr_url);

    let result = SeerrSession::restore(library)
        .link_start()
        .expect("no error surfaces");
    assert_eq!(result["method"], "password");
}

#[test]
fn a_seerr_that_has_not_caught_up_yet_is_polled_rather_than_failed() {
    let (seerr_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("200 OK", r#"{"code":"AB12CD","secret":"s3cret"}"#, &[]),
        response("400 Bad Request", r#"{"message":"not ready"}"#, &[]),
    ]);
    let (jellyfin_url, _) = fake_server(vec![
        response("200 OK", "true", &[]),
        response("200 OK", "true", &[]),
    ]);
    let library = library();
    signed_in_to(&library, &jellyfin_url, "uid");
    configured(&library, &seerr_url);

    let result = SeerrSession::restore(library)
        .link_start()
        .expect("pending, not failed");
    assert_eq!(result["method"], "quickconnect");
    assert_eq!(result["linked"], false);
    assert_eq!(result["secret"], "s3cret");
}

#[test]
fn polling_finishes_the_link_the_start_call_left_open() {
    let (seerr_url, requests) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("200 OK", ME, &[SESSION]),
        response("200 OK", ME, &[]),
        response("200 OK", ME, &[]),
        response("200 OK", QUOTA, &[]),
    ]);
    let library = library();
    signed_in(&library, "uid");
    configured(&library, &seerr_url);
    let session = SeerrSession::restore(library.clone());

    let result = session.link_poll("s3cret").expect("link");
    assert_eq!(result["linked"], true);
    assert_eq!(result["status"]["linked"], true);

    let requests = requests.lock().expect("lock");
    assert!(requests[1].starts_with("POST /api/v1/auth/jellyfin/quickconnect/authenticate"));
    drop(requests);
    assert_eq!(
        library.seerr_config().jellyfin_user_id.as_deref(),
        Some("uid")
    );
}
