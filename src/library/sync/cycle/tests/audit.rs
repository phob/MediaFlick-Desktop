use super::*;
use crate::library::sync::{MAX_INCREMENTAL_PAGES, META_WATERMARK, META_WATERMARK_IDS};

#[test]
fn quitting_during_a_sync_does_not_record_latest_failure() {
    let library = settled_library(&[("m1", "Movie")]);
    set_meta(&library, META_LATEST_FAILURE, "0");
    assert!(ownership_available(&library));

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let (paused_tx, paused_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = receive_target(&listener);
        paused_tx.send(()).expect("signal paused");
        resume_rx.recv().expect("resume");
        send_json(stream, EMPTY_PAGE);
    });

    let session = authenticated_session(&library, &format!("http://{address}"));
    let control = SyncHandle::detached();
    let worker_library = library.clone();
    let worker_control = control.clone();
    let worker = thread::spawn(move || {
        super::super::run_cycle_inner(
            &worker_library,
            &session,
            Trigger::Scheduled,
            Some(&worker_control),
        )
    });

    paused_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("worker reached views request");
    control.stop();
    resume_tx.send(()).expect("release views response");

    let outcome = worker.join().expect("worker");
    server.join().expect("server");
    assert_eq!(outcome.err(), Some(ApiError::Cancelled));
    assert_ne!(
        library.meta(META_LATEST_FAILURE).expect("meta").as_deref(),
        Some("1"),
        "stopping a sync on quit must not record LATEST_FAILURE=1"
    );
    assert!(ownership_available(&library));

    let (failed, _) = cycle(
        &library,
        Trigger::Scheduled,
        vec![ok(EMPTY_PAGE), ok(NOT_JSON)],
    );
    assert!(matches!(failed, Err(ApiError::Decode(_))));
    assert_eq!(
        library.meta(META_LATEST_FAILURE).expect("meta").as_deref(),
        Some("1")
    );
    assert!(!ownership_available(&library));
}

#[test]
fn incremental_page_cap_keeps_the_previous_watermark() {
    let library = settled_library(&[("old-1", "Movie")]);
    let old_watermark = "2024-01-01T00:00:00Z";
    let old_ids = r#"["old-1"]"#;
    set_meta(&library, META_WATERMARK, old_watermark);
    set_meta(&library, META_WATERMARK_IDS, old_ids);

    let full_pages = (0..MAX_INCREMENTAL_PAGES)
        .map(|page| {
            let items = (0..PAGE_SIZE)
                .map(|index| {
                    let seq = page * (PAGE_SIZE as usize) + (index as usize);
                    json!({
                        "Id": format!("cap-{seq:05}"),
                        "Name": format!("Capped {seq}"),
                        "Type": "Movie",
                        "DateCreated": "2025-06-01T00:00:00Z",
                    })
                })
                .collect::<Vec<_>>();
            ok(&json!({
                "Items": items,
                "TotalRecordCount": (MAX_INCREMENTAL_PAGES as i64 + 1) * PAGE_SIZE,
            })
            .to_string())
        })
        .collect::<Vec<_>>();

    let mut replies = vec![ok(EMPTY_PAGE)];
    replies.extend(full_pages);

    let (result, requests) = cycle(&library, Trigger::Scheduled, replies);
    let report = result.expect("capped incremental cycle succeeds");
    assert_eq!(
        steps(&requests)
            .iter()
            .filter(|step| **step == "incremental")
            .count(),
        MAX_INCREMENTAL_PAGES
    );
    assert_eq!(report.updated, MAX_INCREMENTAL_PAGES * (PAGE_SIZE as usize));
    assert_eq!(
        library.meta(META_WATERMARK).expect("watermark").as_deref(),
        Some(old_watermark),
        "hitting the incremental page cap must not advance the watermark past unfetched items"
    );
    assert_eq!(
        library
            .meta(META_WATERMARK_IDS)
            .expect("watermark ids")
            .as_deref(),
        Some(old_ids)
    );
    assert_eq!(
        library
            .meta(META_LAST_BOOTSTRAP)
            .expect("last bootstrap")
            .as_deref(),
        Some("0"),
        "hitting the incremental page cap must mark a full catalog refresh due"
    );
    assert!(
        super::super::full_bootstrap_due(&library).expect("full bootstrap due"),
        "next cycle must schedule a full catalog refresh after a capped incremental walk"
    );
    assert!(
        !first_catalog_download(&library).expect("first download") && ownership_available(&library),
        "scheduling a full catalog refresh must preserve steady-state ownership"
    );

    let full_refresh_page = ok(&json!({
        "Items": [
            {
                "Id": "old-0",
                "Name": "Older",
                "Type": "Movie",
                "DateCreated": "2023-12-31T00:00:00Z"
            },
            {
                "Id": "newest-1",
                "Name": "Newest",
                "Type": "Movie",
                "DateCreated": "2025-07-01T00:00:00Z"
            }
        ],
        "TotalRecordCount": 2,
    })
    .to_string());
    let (result, next_requests) = cycle(
        &library,
        Trigger::Scheduled,
        vec![ok(EMPTY_PAGE), full_refresh_page, ok(EMPTY_PAGE)],
    );
    result.expect("follow-up full catalog refresh succeeds");
    assert_eq!(
        steps(&next_requests),
        ["views", "bootstrap", "incremental"],
        "next cycle after a capped incremental walk must run the full catalog refresh"
    );
    assert_eq!(
        library.meta(META_WATERMARK).expect("watermark").as_deref(),
        Some("2025-07-01T00:00:00Z")
    );
    assert!(!super::super::full_bootstrap_due(&library).expect("full bootstrap due cleared"));
}

#[test]
fn sync_phase_is_set_before_list_libraries_and_resets_between_cycles() {
    let library = settled_library(&[("m1", "Movie")]);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let (views_entered_tx, views_entered_rx) = mpsc::channel();
    let (views_release_tx, views_release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = receive_target(&listener);
        views_entered_tx.send(()).expect("views entered");
        views_release_rx.recv().expect("views release");
        send_json(stream, EMPTY_PAGE);

        let (stream, _) = receive_target(&listener);
        send_json(stream, EMPTY_PAGE);
    });

    let session = authenticated_session(&library, &format!("http://{address}"));
    let generation = session.generation();
    let control = SyncHandle::detached();
    control.set_phase(SyncPhase::Catalog);
    control.running.store(true, Ordering::Relaxed);

    let worker_library = library.clone();
    let worker_control = control.clone();
    let worker = thread::spawn(move || {
        super::super::run_cycle_inner(
            &worker_library,
            &session,
            Trigger::Scheduled,
            Some(&worker_control),
        )
    });

    views_entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("worker reached list_libraries");
    let status = control.status(bootstrap_progress(&library), generation);
    assert_eq!(
        status.progress.phase,
        SyncPhase::Reconciling,
        "phase must be updated before list_libraries instead of keeping the prior cycle's Catalog phase"
    );

    views_release_tx.send(()).expect("release views");
    worker.join().expect("worker").expect("sync cycle");
    server.join().expect("server");

    let lifecycle = SyncHandle::detached();
    let settled = bootstrap_progress(&library);
    assert!(!lifecycle.is_running());
    assert_eq!(lifecycle.stored_phase(), SyncPhase::Complete);

    lifecycle.begin_cycle(SyncPhase::Catalog);
    assert!(lifecycle.is_running());
    assert_eq!(lifecycle.stored_phase(), SyncPhase::Catalog);
    assert_eq!(
        lifecycle.status(settled.clone(), 0).progress.phase,
        SyncPhase::Catalog
    );

    lifecycle.end_cycle();
    assert!(!lifecycle.is_running());
    assert_eq!(lifecycle.stored_phase(), SyncPhase::Complete);
    assert_eq!(
        lifecycle.status(settled.clone(), 0).progress.phase,
        SyncPhase::Complete
    );

    lifecycle.begin_cycle(SyncPhase::Reconciling);
    assert!(lifecycle.is_running());
    assert_eq!(lifecycle.stored_phase(), SyncPhase::Reconciling);
    assert_eq!(
        lifecycle.status(settled, 0).progress.phase,
        SyncPhase::Reconciling
    );
    lifecycle.end_cycle();

    let worker_library = Arc::new(Library::open_in_memory().expect("library"));
    let server2 = serve(vec![ok(EMPTY_PAGE), ok(EMPTY_PAGE), ok(EMPTY_PAGE)]);
    let session2 = Arc::new(authenticated_session(&worker_library, &server2.url));
    let handle = crate::library::sync::spawn(
        worker_library.clone(),
        session2,
        Arc::new(ShellBridge::default()),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if !handle.is_running()
            && worker_library
                .meta("sync.completed_at")
                .expect("meta")
                .is_some()
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        worker_library
            .meta("sync.completed_at")
            .expect("meta")
            .is_some(),
        "worker cycle completed"
    );
    handle.stop();
    let _ = server2.requests();

    assert_eq!(
        handle.stored_phase(),
        SyncPhase::Complete,
        "worker must reset SyncPhase between cycles instead of leaving the prior cycle's phase"
    );
}
