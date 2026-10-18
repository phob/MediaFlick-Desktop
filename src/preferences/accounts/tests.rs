use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::preferences::AppearanceAccent;

static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(1);

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mediaflick-accounts-{label}-{}-{}.json",
        std::process::id(),
        TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn key(server: &str, user: &str) -> AccountKey {
    AccountKey::new(server, user).expect("valid account")
}

fn profile() -> ExternalProfile {
    ExternalProfile {
        id: "0123456789abcdef".to_string(),
        provider: "letterboxd".to_string(),
        profile_key: "alice".to_string(),
        display_name: "Alice".to_string(),
        canonical_url: "https://letterboxd.com/alice/".to_string(),
        enabled: true,
        verification_status: "verified".to_string(),
        created_at: 1,
        last_checked_at: Some(2),
        jellyfin_server_id: String::new(),
        jellyfin_user_id: String::new(),
    }
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(super::super::json_file::backup_path(path));
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
        && let Ok(entries) = std::fs::read_dir(parent)
    {
        let prefix = format!("{}.broken-", name.to_string_lossy());
        for entry in entries.filter_map(Result::ok) {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

#[test]
fn saved_home_gains_a_new_built_in_after_its_predecessor() {
    let path = test_path("home-new-built-in");
    let alice = key("server", "alice");
    let service = AccountConfigurationService::open(path.clone()).expect("open");
    let mut home = HomeSettings::fresh(&["Drama".to_owned()]);
    home.elements.rotate_left(1);
    service.save_home(&alice, &home).expect("save home");
    drop(service);
    let new = HomeElementId::BuiltIn {
        id: HomeBuiltIn::RecentlyAddedShows,
    };
    let predecessor = HomeElementId::BuiltIn {
        id: HomeBuiltIn::RecentlyAdded,
    };
    let mut document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read account settings"))
            .expect("parse account settings");
    document["accounts"][0]["home"]["elements"]
        .as_array_mut()
        .expect("saved Home elements")
        .retain(|element| element["id"] != "recentlyAddedShows");
    crate::preferences::store::atomic_write(&path, document.to_string().as_bytes())
        .expect("write older Home");

    let reopened = AccountConfigurationService::open(path.clone()).expect("reopen");
    let elements = reopened.home(&alice).expect("home").elements;
    let position = |id: &HomeElementId| {
        elements
            .iter()
            .position(|element| &element.element == id)
            .expect("Home element")
    };
    assert_eq!(position(&new), position(&predecessor) + 1);
    assert!(elements[position(&new)].enabled);
    let user_order = elements
        .iter()
        .filter(|element| element.element != new)
        .collect::<Vec<_>>();
    let saved_order = home
        .elements
        .iter()
        .filter(|element| element.element != new)
        .collect::<Vec<_>>();
    assert_eq!(user_order, saved_order);
    cleanup(&path);
}

#[test]
fn library_home_elements_round_trip_and_reject_non_canonical_ids() {
    const ID: &str = "f137a2dd21bbc1b99aa5c0f6bf02a805";
    let wire = serde_json::json!({ "kind": "library", "id": ID, "enabled": false });
    let element = serde_json::from_value::<HomeElement>(wire.clone()).expect("library element");
    assert_eq!(
        element,
        HomeElement {
            element: HomeElementId::Library {
                id: LibraryId::canonical(ID).expect("canonical id"),
            },
            enabled: false,
        }
    );
    assert_eq!(serde_json::to_value(&element).expect("serialize"), wire);
    for id in [
        ID.to_ascii_uppercase(),
        format!(" {ID}"),
        "f137a2dd".to_owned(),
        "../library".to_owned(),
    ] {
        let wire = serde_json::json!({ "kind": "library", "id": id, "enabled": true });
        assert!(
            serde_json::from_value::<HomeElement>(wire).is_err(),
            "{id:?} must not load"
        );
    }
}

#[test]
fn browsing_survives_reopen_and_rejects_external_destinations() {
    let path = test_path("browsing-round-trip");
    let alice = key("server", "alice");
    let bob = key("server", "bob");
    let service = AccountConfigurationService::open(path.clone()).expect("open");
    service
        .save_browsing(&alice, "last", "/library?kind=Movie&sort=year")
        .expect("save browsing");
    assert!(
        service
            .save_browsing(&alice, "last", "https://example.com")
            .is_err()
    );
    drop(service);
    let reopened = AccountConfigurationService::open(path).expect("reopen");
    assert!(reopened.browsing(&bob).is_empty());
    assert_eq!(
        reopened.browsing(&alice)["last"],
        "/library?kind=Movie&sort=year"
    );
}

const LIBRARY: &str = "f137a2dd21bbc1b99aa5c0f6bf02a805";
const OTHER_LIBRARY: &str = "7e64e319657a9516ec78490da03edccb";

fn write_browsing(path: &Path, account: usize, browsing: serde_json::Value) {
    let mut document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read account settings"))
            .expect("parse account settings");
    document["accounts"][account]["browsing"] = browsing;
    crate::preferences::store::atomic_write(path, document.to_string().as_bytes())
        .expect("write browsing");
}

#[test]
fn library_browsing_keys_hold_only_their_own_library_route() {
    let path = test_path("browsing-library-keys");
    let alice = key("server", "alice");
    let service = AccountConfigurationService::open(path.clone()).expect("open");
    let page = format!("library:{LIBRARY}");
    let route = format!("/library/{LIBRARY}?sort=year&genre=Drama");
    service
        .save_browsing(&alice, &page, &route)
        .expect("save a library's filters");
    service
        .save_browsing(&alice, "last", &format!("/library/{LIBRARY}?sort=added"))
        .expect("save a library page as the last route");

    let rejected = [
        (page.clone(), format!("/library/{OTHER_LIBRARY}?sort=year")),
        (page.clone(), "/library?kind=Movie&sort=year".to_string()),
        (page.clone(), format!("/library/{LIBRARY}/extra")),
        (
            page.clone(),
            format!("https://example.com/library/{LIBRARY}"),
        ),
        (
            "library:not-hex".to_string(),
            "/library/not-hex".to_string(),
        ),
        ("library:".to_string(), "/library/".to_string()),
        (
            format!("library:{}", LIBRARY.to_ascii_uppercase()),
            format!("/library/{}", LIBRARY.to_ascii_uppercase()),
        ),
        ("Movie".to_string(), "/library?kind=Movie".to_string()),
        ("Series".to_string(), "/library?kind=Series".to_string()),
        ("Music".to_string(), format!("/library/{LIBRARY}")),
        ("last".to_string(), "/library/not-hex".to_string()),
    ];
    for (page, route) in rejected {
        assert!(
            service.save_browsing(&alice, &page, &route).is_err(),
            "{page} -> {route} must be rejected"
        );
    }
    drop(service);

    let reopened = AccountConfigurationService::open(path.clone()).expect("reopen");
    let browsing = reopened.browsing(&alice);
    assert_eq!(browsing[&page], route);
    assert_eq!(browsing.len(), 2);
    cleanup(&path);
}

#[test]
fn legacy_browsing_keys_load_unchanged_and_leave_with_the_next_library_save() {
    let path = test_path("browsing-legacy-keys");
    let alice = key("server", "alice");
    let bob = key("server", "bob");
    let service = AccountConfigurationService::open(path.clone()).expect("open");
    service.save_browsing(&alice, "last", "/").expect("alice");
    service.save_browsing(&bob, "last", "/").expect("bob");
    drop(service);
    let legacy = serde_json::json!({
        "last": "/library?kind=Movie&sort=year",
        "Movie": "/library?kind=Movie&sort=year",
        "Series": "/library?kind=Series&watched=unwatched",
    });
    write_browsing(&path, 0, legacy.clone());
    write_browsing(&path, 1, legacy);
    let written = std::fs::read(&path).expect("read legacy file");

    let reopened = AccountConfigurationService::open(path.clone()).expect("legacy keys load");
    assert_eq!(
        reopened.browsing(&alice)["Movie"],
        "/library?kind=Movie&sort=year"
    );
    assert_eq!(
        std::fs::read(&path).expect("read after open"),
        written,
        "opening must not rewrite the file"
    );

    reopened
        .save_browsing(&alice, "last", "/calendar")
        .expect("save the last route");
    assert!(
        reopened.browsing(&alice).contains_key("Movie"),
        "navigating keeps the legacy pages until a library's filters are saved"
    );
    let page = format!("library:{LIBRARY}");
    reopened
        .save_browsing(&alice, &page, &format!("/library/{LIBRARY}?sort=year"))
        .expect("save a library's filters");
    let browsing = reopened.browsing(&alice);
    assert!(!browsing.contains_key("Movie"));
    assert!(!browsing.contains_key("Series"));
    assert_eq!(browsing["last"], "/calendar");
    assert!(browsing.contains_key(&page));
    assert!(reopened.browsing(&bob).contains_key("Movie"));
    drop(reopened);

    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read saved file"))
            .expect("parse saved file");
    assert!(saved["accounts"][0]["browsing"].get("Movie").is_none());
    assert!(saved["accounts"][0]["browsing"].get("Series").is_none());
    assert!(saved["accounts"][1]["browsing"].get("Movie").is_some());
    cleanup(&path);
}

#[test]
fn saving_a_route_a_page_already_holds_leaves_the_file_alone() {
    let path = test_path("browsing-unchanged");
    let alice = key("server", "alice");
    let bob = key("server", "bob");
    let service = AccountConfigurationService::open(path.clone()).expect("open");
    let page = format!("library:{LIBRARY}");
    let route = format!("/library/{LIBRARY}?sort=year");
    service.save_browsing(&alice, "last", &route).expect("last");
    service
        .save_browsing(&alice, &page, &route)
        .expect("library");
    let append_rewrite_marker = || {
        let mut bytes = std::fs::read(&path).expect("read account settings");
        bytes.extend_from_slice(b"\n\n");
        crate::preferences::store::atomic_write(&path, &bytes).expect("mark account settings");
        bytes
    };

    let marked = append_rewrite_marker();
    service
        .save_browsing(&alice, "last", &route)
        .expect("the same last route");
    service
        .save_browsing(&alice, &page, &route)
        .expect("the same library filters");
    assert_eq!(
        std::fs::read(&path).expect("read after unchanged saves"),
        marked,
        "an unchanged browsing save leaves accounts.json as it was"
    );

    service
        .save_browsing(&bob, "last", &route)
        .expect("another account's route");
    assert_ne!(std::fs::read(&path).expect("read"), marked);
    assert_eq!(service.browsing(&bob)["last"], route);

    let marked = append_rewrite_marker();
    service
        .save_browsing(&alice, "last", "/calendar")
        .expect("a new last route");
    assert_ne!(std::fs::read(&path).expect("read"), marked);
    assert_eq!(service.browsing(&alice)["last"], "/calendar");
    drop(service);

    write_browsing(
        &path,
        0,
        serde_json::json!({
            "last": "/calendar",
            "Movie": "/library?kind=Movie&sort=year",
            page.clone(): route,
        }),
    );
    let reopened = AccountConfigurationService::open(path.clone()).expect("reopen");
    assert!(
        reopened
            .save_browsing(&alice, "Movie", "/library?kind=Movie&sort=year")
            .is_err(),
        "a legacy page stays unwritable even with the value it holds"
    );
    reopened
        .save_browsing(&alice, &page, &route)
        .expect("the same library filters with a legacy page");
    assert!(!reopened.browsing(&alice).contains_key("Movie"));
    drop(reopened);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read saved file"))
            .expect("parse saved file");
    assert!(
        saved["accounts"][0]["browsing"].get("Movie").is_none(),
        "a due legacy prune still writes the file"
    );
    cleanup(&path);
}

#[test]
fn browsing_holds_at_most_256_pages() {
    let path = test_path("browsing-bound");
    let alice = key("server", "alice");
    let service = AccountConfigurationService::open(path.clone()).expect("open");
    service.save_browsing(&alice, "last", "/").expect("alice");
    drop(service);
    let library = |index: usize| format!("{index:032x}");
    let pages = |count: usize| {
        let mut browsing = serde_json::Map::new();
        browsing.insert("last".to_string(), "/".into());
        for index in 1..count {
            browsing.insert(
                format!("library:{}", library(index)),
                format!("/library/{}", library(index)).into(),
            );
        }
        serde_json::Value::Object(browsing)
    };

    write_browsing(&path, 0, pages(257));
    let oversized = std::fs::read(&path).expect("read oversized file");
    assert!(AccountConfigurationService::open(path.clone()).is_err());
    assert_eq!(
        std::fs::read(&path).expect("read after the refusal"),
        oversized,
        "a refused file stays as it was"
    );

    write_browsing(&path, 0, pages(256));
    let service = AccountConfigurationService::open(path.clone()).expect("256 pages load");
    assert!(
        service
            .save_browsing(
                &alice,
                &format!("library:{}", library(256)),
                &format!("/library/{}", library(256)),
            )
            .is_err(),
        "a 257th page is refused"
    );
    service
        .save_browsing(
            &alice,
            &format!("library:{}", library(1)),
            &format!("/library/{}?sort=year", library(1)),
        )
        .expect("replacing a page at the bound");
    assert_eq!(service.browsing(&alice).len(), 256);
    cleanup(&path);
}

#[test]
fn account_settings_are_isolated_and_survive_reopen() {
    let path = test_path("round-trip");
    let alice = key("server", "alice");
    let bob = key("server", "bob");
    let service = AccountConfigurationService::open(path.clone()).expect("open");
    let appearance = AppearanceSettings {
        accent: AppearanceAccent::Violet,
        ..AppearanceSettings::default()
    };
    service
        .save_appearance(&alice, &appearance)
        .expect("save appearance");
    service
        .save_letterboxd_profile(&alice, &profile())
        .expect("save profile");
    let home = HomeSettings::fresh(&["Action".to_owned(), "Comedy".to_owned()]);
    service.save_home(&alice, &home).expect("save home");

    assert_eq!(service.appearance(&bob), AppearanceSettings::default());
    assert!(service.letterboxd_profiles(&bob).is_empty());
    assert_eq!(service.home(&bob), None);
    drop(service);

    let reopened = AccountConfigurationService::open(path.clone()).expect("reopen");
    assert_eq!(reopened.appearance(&alice), appearance);
    assert_eq!(reopened.home(&alice), Some(home));
    let profiles = reopened.letterboxd_profiles(&alice);
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].jellyfin_server_id, "server");
    assert_eq!(profiles[0].jellyfin_user_id, "alice");
    cleanup(&path);
}

#[test]
fn signed_out_legacy_appearance_waits_for_the_next_account() {
    let path = test_path("pending-appearance");
    let appearance = AppearanceSettings {
        accent: AppearanceAccent::Violet,
        ..AppearanceSettings::default()
    };
    let alice = key("server", "alice");
    let service = AccountConfigurationService::open(path.clone()).expect("open");

    service
        .import_legacy_appearance(None, &appearance)
        .expect("stage signed-out appearance");
    drop(service);

    let reopened = AccountConfigurationService::open(path.clone()).expect("reopen");
    assert_eq!(reopened.appearance(&alice), AppearanceSettings::default());
    reopened
        .claim_legacy_appearance(&alice)
        .expect("claim appearance");
    assert_eq!(reopened.appearance(&alice), appearance);
    reopened
        .claim_legacy_appearance(&key("server", "bob"))
        .expect("idempotent retry");
    assert_eq!(reopened.appearance(&alice), appearance);
    cleanup(&path);
}

#[test]
fn adding_the_same_letterboxd_username_preserves_its_identity() {
    let path = test_path("profile-upsert");
    let alice = key("server", "alice");
    let service = AccountConfigurationService::open(path.clone()).expect("open");
    let original = service
        .save_letterboxd_profile(&alice, &profile())
        .expect("save original profile");
    let mut replacement = profile();
    replacement.id = "fedcba9876543210".to_string();
    replacement.display_name = "Alice Updated".to_string();
    replacement.created_at = 99;

    let updated = service
        .save_letterboxd_profile(&alice, &replacement)
        .expect("update profile");

    assert_eq!(updated.id, original.id);
    assert_eq!(updated.created_at, original.created_at);
    assert_eq!(updated.display_name, "Alice Updated");
    assert_eq!(service.letterboxd_profiles(&alice), vec![updated]);
    cleanup(&path);
}
