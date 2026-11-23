use super::*;

fn seed_library_and_manual_profile(fixture: &TestServices) -> (AccountKey, CollectionProfile) {
    let (account, mut profile) = seed_library_and_profile(fixture);
    profile.cadence = RefreshCadence::Manual;
    fixture
        .services
        .collections
        .save_profile(&account, profile.clone())
        .expect("save manual profile");
    (account, profile)
}

#[test]
fn uninitialized_manual_profile_is_populated_automatically_and_respects_failure_backoff() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1", "franchise-memberships-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "503 Service Unavailable",
        results_body: "{}".to_string(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-manual-init");
    let (account, profile) = seed_library_and_manual_profile(&fixture);
    let shell_rx = fixture.services.shell.subscribe();

    fixture.services.companion.probe(true).expect("probe");
    drain_collections_changed(&shell_rx);

    let initial = browse::profile_detail(&fixture.services, &profile.id).expect("detail");
    let initial_json = serde_json::to_value(&initial).expect("json");
    assert_eq!(initial_json["status"], "updating");
    assert_eq!(initial_json["overdue"], true);

    assert!(
        wait_for_collections_changed(&shell_rx, Duration::from_secs(5)),
        "opening an uninitialized manual collection must trigger initial population"
    );
    assert_eq!(server.results_hits(), 1);

    let failed = browse::profile_detail(&fixture.services, &profile.id).expect("failed detail");
    let failed_json = serde_json::to_value(&failed).expect("json");
    assert_eq!(failed_json["status"], "resultsUnavailable");
    assert_eq!(failed_json["overdue"], true);

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(
        server.results_hits(),
        1,
        "uninitialized manual profile failure must respect backoff"
    );

    let repository = SnapshotRepository::new(&fixture.services.library);
    repository
        .clear_failure_backoff(&account)
        .expect("clear backoff");
    server.update(|behavior| {
        behavior.results_status = "200 OK";
        behavior.results_body = profile_results_json();
    });

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(server.results_hits(), 2);
    let ready = browse::profile_detail(&fixture.services, &profile.id).expect("ready detail");
    let ready_json = serde_json::to_value(&ready).expect("json");
    assert_eq!(ready_json["status"], "ready");
    assert_eq!(ready_json["overdue"], false);

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(
        server.results_hits(),
        2,
        "initialized manual profile must not refresh automatically again"
    );
}

#[test]
fn concurrent_profile_refreshes_coalesce_per_account_and_profile() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1", "franchise-memberships-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-concurrent");
    let (account, profile) = seed_library_and_profile(&fixture);
    fixture.services.companion.probe(true).expect("probe");

    let (entered_rx, release_tx) = server.pause_next_results();
    let services_one = fixture.services.clone();
    let first = std::thread::spawn(move || {
        crate::collections::scheduler::refresh_due_work(&services_one);
    });

    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("first refresh reached companion");

    let services_two = fixture.services.clone();
    let second = std::thread::spawn(move || {
        crate::collections::scheduler::refresh_due_work(&services_two);
    });
    second
        .join()
        .expect("second refresh finished without blocking");

    assert_eq!(
        server.results_hits(),
        1,
        "a concurrent pass must not refresh a profile already in flight"
    );

    release_tx.send(()).expect("release companion");
    first.join().expect("first refresh finished");

    let repository = SnapshotRepository::new(&fixture.services.library);
    let mut state = repository
        .refresh_state(&account, &profile.id)
        .expect("state");
    state.last_success = Some(1);
    repository
        .save_refresh_state(&account, &profile.id, &state)
        .expect("make profile due again");

    crate::collections::scheduler::refresh_due_work(&fixture.services);
    assert_eq!(
        server.results_hits(),
        2,
        "profile flight guard must release after completion"
    );
}

#[test]
fn scheduler_does_not_write_orphan_rows_when_account_switches_or_profile_deleted_mid_refresh() {
    let server = LoopbackCompanion::start(CompanionBehavior {
        info_status: "200 OK",
        info_body: info_json(&["collection-experience-v1", "franchise-memberships-v1"]),
        franchises_status: "200 OK",
        franchises_body: franchise_success_json(),
        results_status: "200 OK",
        results_body: profile_results_json(),
    });
    let fixture = TestServices::signed_in_as(&server.url, "user-gate-switch");
    let (account, profile) = seed_library_and_profile(&fixture);
    fixture.services.companion.probe(true).expect("probe");

    let (entered_rx, release_tx) = server.pause_next_results();
    let services = fixture.services.clone();
    let worker = std::thread::spawn(move || {
        crate::collections::scheduler::refresh_due_work(&services);
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("refresh reached companion");

    fixture.services.session.clear_local(false).expect("logout");
    release_tx.send(()).expect("release companion");
    worker.join().expect("worker joined");

    let repository = SnapshotRepository::new(&fixture.services.library);
    assert!(
        repository
            .profile(&account, &profile.id, &profile.revision)
            .expect("profile snapshot")
            .is_none(),
        "stale scheduler refresh must not write a snapshot after sign out"
    );
    let state = repository
        .refresh_state(&account, &profile.id)
        .expect("refresh state");
    assert!(
        !state.initialized && state.last_success.is_none() && state.last_attempt.is_none(),
        "stale scheduler refresh must not write refresh state after sign out"
    );

    let fixture_del = TestServices::signed_in_as(&server.url, "user-gate-delete");
    let (del_account, del_profile) = seed_library_and_profile(&fixture_del);
    fixture_del.services.companion.probe(true).expect("probe");
    server.update(|behavior| {
        behavior.results_status = "503 Service Unavailable";
        behavior.results_body = "{}".to_string();
    });

    let (entered_rx, release_tx) = server.pause_next_results();
    let services_del = fixture_del.services.clone();
    let worker_del = std::thread::spawn(move || {
        crate::collections::scheduler::refresh_due_work(&services_del);
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("failed refresh reached companion");

    profiles::delete(&fixture_del.services, None, &del_profile.id).expect("delete profile");
    release_tx.send(()).expect("release companion");
    worker_del.join().expect("delete worker joined");

    let del_repo = SnapshotRepository::new(&fixture_del.services.library);
    let del_state = del_repo
        .refresh_state(&del_account, &del_profile.id)
        .expect("refresh state");
    assert!(
        del_state.last_attempt.is_none() && del_state.latest_failure.is_none(),
        "failed refresh must not write orphan refresh state after profile deletion"
    );

    let fixture_ok_del = TestServices::signed_in_as(&server.url, "user-gate-delete-ok");
    let (ok_account, ok_profile) = seed_library_and_profile(&fixture_ok_del);
    fixture_ok_del
        .services
        .companion
        .probe(true)
        .expect("probe");
    server.update(|behavior| {
        behavior.results_status = "200 OK";
        behavior.results_body = profile_results_json();
    });

    let (entered_rx, release_tx) = server.pause_next_results();
    let services_ok_del = fixture_ok_del.services.clone();
    let worker_ok_del = std::thread::spawn(move || {
        crate::collections::scheduler::refresh_due_work(&services_ok_del);
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("successful refresh reached companion");

    profiles::delete(&fixture_ok_del.services, None, &ok_profile.id).expect("delete profile");
    release_tx.send(()).expect("release companion");
    worker_ok_del.join().expect("ok delete worker joined");

    let ok_repo = SnapshotRepository::new(&fixture_ok_del.services.library);
    assert!(
        ok_repo
            .profile(&ok_account, &ok_profile.id, &ok_profile.revision)
            .expect("profile snapshot")
            .is_none(),
        "successful refresh must not write orphan snapshot after profile deletion"
    );
    let ok_state = ok_repo
        .refresh_state(&ok_account, &ok_profile.id)
        .expect("refresh state");
    assert!(
        !ok_state.initialized && ok_state.last_success.is_none() && ok_state.last_attempt.is_none(),
        "successful refresh must not write orphan refresh state after profile deletion"
    );
}
