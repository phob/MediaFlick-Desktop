use super::support::*;
use super::*;

#[test]
fn a_fresh_session_is_neither_configured_nor_linked() {
    let session = SeerrSession::restore(library());
    let status = session.status();
    assert_eq!(status["configured"], false);
    assert_eq!(status["linked"], false);
    assert_eq!(status["user"], serde_json::Value::Null);
    assert!(matches!(
        session.client(),
        Err(super::SeerrError::NotConfigured)
    ));
}

#[test]
fn restoring_reads_the_persisted_link() {
    let library = library();
    signed_in(&library, "uid");
    linked(&library, "uid");

    let session = SeerrSession::restore(library);
    let client = session.client().expect("client");
    assert_eq!(
        client.url("auth/me", &[]),
        "https://seerr.test/api/v1/auth/me"
    );
    assert!(client.cookies().has_session());
}

#[test]
fn an_unusable_address_is_rejected_before_any_request() {
    let session = SeerrSession::restore(library());
    assert!(matches!(
        session.connect("file:///etc/passwd"),
        Err(super::SeerrError::NotConfigured)
    ));
    assert!(matches!(
        session.connect("javascript:alert(1)"),
        Err(super::SeerrError::NotConfigured)
    ));
    assert!(matches!(
        session.connect("  "),
        Err(super::SeerrError::NotConfigured)
    ));
}

#[test]
fn switching_jellyfin_user_drops_the_link_from_memory_and_disk() {
    let library = library();
    signed_in(&library, "uid");
    linked(&library, "uid");
    let session = SeerrSession::restore(library.clone());
    assert!(is_linked(&session));

    // Somebody else signs in on the same machine.
    signed_in(&library, "other-uid");

    assert!(!is_linked(&session));
    assert_eq!(session.status()["linked"], false);
    let stored = library.seerr_config();
    assert_eq!(stored.cookies, None);
    assert_eq!(stored.user_id, None);
    // The instance address survives, so re-linking needs no retyping.
    assert_eq!(stored.base_url.as_deref(), Some("https://seerr.test"));
}

#[test]
fn signing_out_of_jellyfin_drops_the_link() {
    let library = library();
    signed_in(&library, "uid");
    linked(&library, "uid");
    let session = SeerrSession::restore(library.clone());

    library.clear_session(false).expect("sign out");

    assert!(!is_linked(&session));
    assert_eq!(library.seerr_config().cookies, None);
}

#[test]
fn a_link_made_against_another_jellyfin_server_is_dropped() {
    let library = library();
    signed_in(&library, "uid");
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some("https://seerr.test".to_string()),
            cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
            jellyfin_server_id: Some("a-different-server".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            ..SeerrConfig::default()
        })
        .expect("seerr config");

    let session = SeerrSession::restore(library.clone());
    assert!(!is_linked(&session));
    assert_eq!(library.seerr_config().cookies, None);
}

#[test]
fn connecting_captures_the_csrf_pair_from_the_very_first_probe() {
    let (base_url, requests) = fake_server(vec![
        response(
            "200 OK",
            INITIALIZED,
            &[
                "_csrf=secret; Path=/; HttpOnly; SameSite=Strict",
                "XSRF-TOKEN=token123; Path=/",
            ],
        ),
        response("200 OK", VERSION, &[]),
    ]);
    let library = library();
    let session = SeerrSession::restore(library.clone());

    let result = session.connect(&base_url).expect("connect");
    assert_eq!(result["serverUrl"], base_url);
    assert_eq!(result["version"], "3.3.0");
    assert_eq!(result["partialRequestsEnabled"], true);
    assert_eq!(result["linked"], false);

    let requests = requests.lock().expect("lock");
    assert!(requests[0].starts_with("GET /api/v1/settings/public HTTP/1.1"));
    assert!(requests[1].starts_with("GET /api/v1/status HTTP/1.1"));
    // The pair the probe handed out is already on the second request, and
    // is persisted so the first write after a restart still has it.
    assert!(requests[1].contains("cookie: XSRF-TOKEN=token123; _csrf=secret"));
    drop(requests);
    let stored = library.seerr_config();
    assert_eq!(stored.base_url.as_deref(), Some(base_url.as_str()));
    assert!(
        stored
            .cookies
            .as_deref()
            .is_some_and(|cookies| cookies.contains("XSRF-TOKEN"))
    );
    assert!(stored.partial_requests_enabled);
}

#[test]
fn an_uninitialized_instance_is_refused_before_it_can_be_set_up_by_accident() {
    let (base_url, _) = fake_server(vec![response(
        "200 OK",
        r#"{"initialized":false,"plexClientIdentifier":"abc"}"#,
        &[],
    )]);
    let library = library();
    let session = SeerrSession::restore(library.clone());

    let error = session.connect(&base_url).expect_err("refused");
    assert!(matches!(error, super::SeerrError::Unusable(_)));
    assert!(error.to_string().contains("setup wizard"));
    assert_eq!(library.seerr_config(), SeerrConfig::default());
}

/// The commonest way a Seerr address goes wrong: it reaches a proxy, a
/// sign-on page, or Seerr's own web front end, all of which answer 200 with
/// HTML. A JSON parser position for that would send the user hunting for a
/// fault in Seerr rather than in what they typed.
#[test]
fn an_address_that_answers_with_a_web_page_says_so() {
    let (base_url, _) = fake_server(vec![html_response(
        "200 OK",
        "<!DOCTYPE html>\n<html>\n<head>\n<title>Sign in</title>\n</head>\n<body>x</body>\n</html>",
    )]);
    let session = SeerrSession::restore(library());

    let error = session.connect(&base_url).expect_err("refused");
    assert!(matches!(error, SeerrError::Unusable(_)), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("a web page"), "{message}");
    // Never a parser position: that is a fact about our decoder, not about
    // the address the user has to fix.
    assert!(!message.contains("line"), "{message}");
}

/// An address behind a sign-on proxy — authentik, Authelia, oauth2-proxy,
/// Cloudflare Access — never reaches Seerr at all. Following the redirect
/// would land on a login page and fail somewhere deep in it, reporting a
/// fault that says nothing about the real problem.
#[test]
fn an_address_behind_a_sign_on_proxy_is_named_rather_than_chased() {
    let (base_url, requests) = fake_server(vec![
        redirect_response("https://auth.example.de/application/o/authorize/?state=jwt"),
        // Never reached: chasing this is exactly what must not happen.
        response("200 OK", INITIALIZED, &[]),
    ]);
    let session = SeerrSession::restore(library());

    let error = session.connect(&base_url).expect_err("refused");
    assert!(matches!(error, SeerrError::Unusable(_)), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("auth.example.de"), "{message}");
    assert!(message.contains("sign-on proxy"), "{message}");

    assert_eq!(
        requests.lock().expect("lock").len(),
        1,
        "the redirect was followed"
    );
}

/// A body that really is JSON but the wrong shape is a different fault, and
/// must not be reported as a wrong address.
#[test]
fn a_json_body_of_the_wrong_shape_stays_a_decode_failure() {
    let (base_url, _) = fake_server(vec![response("200 OK", "[1, 2, 3]", &[])]);
    let session = SeerrSession::restore(library());
    assert!(matches!(
        session.connect(&base_url),
        Err(SeerrError::Decode(_))
    ));
}

#[test]
fn an_instance_wired_to_another_media_server_is_refused() {
    let (base_url, _) = fake_server(vec![response(
        "200 OK",
        r#"{"initialized":true,"mediaServerType":1}"#,
        &[],
    )]);
    let session = SeerrSession::restore(library());
    let error = session.connect(&base_url).expect_err("refused");
    assert!(error.to_string().contains("Jellyfin"));
}

#[test]
fn a_failing_version_probe_does_not_fail_the_connect() {
    let (base_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("404 Not Found", "{}", &[]),
    ]);
    let session = SeerrSession::restore(library());
    let result = session.connect(&base_url).expect("connect");
    assert_eq!(result["version"], "");
}

#[test]
fn moving_to_another_instance_logs_the_old_session_out_with_its_csrf_header() {
    let (old_url, old_requests) = fake_server(vec![response("204 No Content", "", &[])]);
    let (new_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("200 OK", VERSION, &[]),
    ]);
    let library = library();
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some(old_url),
            cookies: Some(r#"{"XSRF-TOKEN":"token123","connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            ..SeerrConfig::default()
        })
        .expect("seerr config");
    let session = SeerrSession::restore(library.clone());

    session.connect(&new_url).expect("connect");

    let logout = old_requests.lock().expect("lock");
    assert_eq!(logout.len(), 1, "the old instance was not logged out");
    assert!(logout[0].starts_with("POST /api/v1/auth/logout HTTP/1.1"));
    assert!(logout[0].contains("cookie: XSRF-TOKEN=token123; connect.sid=abc"));
    // csurf accepts the token echoed as a header; without it a
    // CSRF-protected instance rejects every write.
    assert!(logout[0].contains("x-xsrf-token: token123"));
    drop(logout);

    // Nothing of the old link survives the move.
    let stored = library.seerr_config();
    assert_eq!(stored.base_url.as_deref(), Some(new_url.as_str()));
    assert_eq!(stored.user_id, None);
    assert!(
        stored
            .cookies
            .as_deref()
            .is_none_or(|cookies| !cookies.contains("connect.sid"))
    );
}

#[test]
fn re_probing_the_same_instance_keeps_the_session_and_refreshes_the_csrf_pair() {
    let (base_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &["XSRF-TOKEN=rotated; Path=/"]),
        response("200 OK", VERSION, &[]),
    ]);
    let library = library();
    signed_in(&library, "uid");
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some(base_url.clone()),
            cookies: Some(r#"{"XSRF-TOKEN":"stale","connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            ..SeerrConfig::default()
        })
        .expect("seerr config");
    let session = SeerrSession::restore(library.clone());

    let result = session.connect(&base_url).expect("connect");
    assert_eq!(result["linked"], true);

    let stored = library.seerr_config();
    assert_eq!(stored.user_id, Some(7));
    let cookies = stored.cookies.expect("cookies");
    assert!(cookies.contains(r#""connect.sid":"abc""#));
    assert!(cookies.contains(r#""XSRF-TOKEN":"rotated""#));
    assert!(is_linked(&session));
}

#[test]
fn re_probing_after_an_account_switch_does_not_preserve_the_previous_link() {
    let (base_url, _) = fake_server(vec![
        response("200 OK", INITIALIZED, &[]),
        response("200 OK", VERSION, &[]),
    ]);
    let library = library();
    signed_in(&library, "uid");
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some(base_url.clone()),
            cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            ..SeerrConfig::default()
        })
        .expect("seerr config");
    let session = SeerrSession::restore(library.clone());

    signed_in(&library, "other-uid");
    let result = session.connect(&base_url).expect("connect");

    assert_eq!(result["linked"], false);
    let stored = library.seerr_config();
    assert_eq!(stored.base_url.as_deref(), Some(base_url.as_str()));
    assert_eq!(stored.cookies, None);
    assert_eq!(stored.user_id, None);
    assert_eq!(stored.jellyfin_user_id, None);
}

#[test]
fn a_stale_probe_cannot_overwrite_a_newer_stored_link() {
    let library = library();
    signed_in(&library, "uid");
    linked(&library, "uid");
    let session = SeerrSession::restore(library.clone());
    let stale_revision = session.read().revision;
    let stale_client = SeerrClient::new(
        "https://seerr.test",
        SessionCookies::from_json(r#"{"connect.sid":"stale"}"#),
    );

    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some("https://new.test".to_string()),
            cookies: Some(r#"{"connect.sid":"new"}"#.to_string()),
            user_id: Some(99),
            user_name: Some("new user".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some("uid".to_string()),
            ..SeerrConfig::default()
        })
        .expect("new link");

    assert!(!session.absorb_probe(&stale_client, stale_revision));
    let stored = library.seerr_config();
    assert_eq!(stored.base_url.as_deref(), Some("https://new.test"));
    assert_eq!(stored.user_id, Some(99));
    assert!(
        stored
            .cookies
            .as_deref()
            .is_some_and(|cookies| cookies.contains(r#""connect.sid":"new""#))
    );
    assert_eq!(session.read().base_url.as_deref(), Some("https://new.test"));
}

#[test]
fn a_configured_but_unlinked_instance_reports_its_switches() {
    let library = library();
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some("https://seerr.test".to_string()),
            movie_4k_enabled: true,
            partial_requests_enabled: true,
            ..SeerrConfig::default()
        })
        .expect("seerr config");

    let status = SeerrSession::restore(library).status();
    assert_eq!(status["configured"], true);
    assert_eq!(status["linked"], false);
    assert_eq!(status["serverUrl"], "https://seerr.test");
    assert_eq!(status["instance"]["movie4kEnabled"], true);
    assert_eq!(status["instance"]["series4kEnabled"], false);
    assert_eq!(status["instance"]["partialRequestsEnabled"], true);
}

#[test]
fn the_state_round_trips_through_its_stored_form() {
    let config = SeerrConfig {
        base_url: Some("https://seerr.test".to_string()),
        cookies: Some(r#"{"XSRF-TOKEN":"t","connect.sid":"abc"}"#.to_string()),
        user_id: Some(7),
        user_name: Some("pho".to_string()),
        jellyfin_server_id: Some("srv".to_string()),
        jellyfin_user_id: Some("uid".to_string()),
        movie_4k_enabled: true,
        series_4k_enabled: true,
        partial_requests_enabled: true,
    };
    assert_eq!(
        SeerrState::from_config(config.clone(), 42).to_config(),
        config
    );
}

#[test]
fn an_empty_cookie_jar_is_stored_as_no_cookies_at_all() {
    let state = SeerrState::from_config(
        SeerrConfig {
            base_url: Some("https://seerr.test".to_string()),
            ..SeerrConfig::default()
        },
        0,
    );
    assert_eq!(state.to_config().cookies, None);
}

#[test]
fn a_poisoned_state_lock_keeps_the_existing_state() {
    let session = SeerrSession::restore(library());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut state = session.state.write().expect("state");
        state.base_url = Some("https://seerr.test".to_string());
        drop(state);
        panic!("poison the state lock");
    }));

    assert_eq!(
        session.read().base_url.as_deref(),
        Some("https://seerr.test")
    );
}
