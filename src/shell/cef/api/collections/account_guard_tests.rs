use std::net::TcpListener;

use serde_json::Value;

use super::super::tests::{login_as, post, send, server_url};
use super::*;
use crate::app::services::test_support::TestServices;
use crate::collections::{
    CollectionMode, CollectionProfile, CollectionSource, MediaType, RefreshCadence, ResultLimit,
    TemplateReference,
};
use crate::preferences::{AccountKey, CollectionConfigurationService};

const ID_A: &str = "aaaaaaaaaaaaaaaa";
const ID_B: &str = "bbbbbbbbbbbbbbbb";

fn sample_profile(id: &str, title: &str) -> CollectionProfile {
    CollectionProfile {
        id: id.to_string(),
        revision: "c".repeat(16),
        template: TemplateReference {
            id: "tmdb.discover.movie.popular".to_string(),
        },
        title: title.to_string(),
        description: String::new(),
        custom_poster_id: None,
        source: CollectionSource::TmdbDiscover {
            parameters: Default::default(),
        },
        media_type: MediaType::Movie,
        limit: ResultLimit::All,
        cadence: RefreshCadence::Daily,
        available_on_home: false,
    }
}

#[test]
fn collection_mutations_carrying_previous_account_reject_with_409_after_account_switch() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let url = server_url(&listener);
    let fixture = TestServices::signed_in_as(&url, "alice");
    let bob = AccountKey::new("server", "bob").expect("bob");

    fixture
        .services
        .collections
        .save_profile(&bob, sample_profile(ID_A, "Bob One"))
        .expect("bob one");
    fixture
        .services
        .collections
        .save_profile(&bob, sample_profile(ID_B, "Bob Two"))
        .expect("bob two");

    login_as(&fixture, listener, &url, "bob", "Bob");
    let expected = crate::app::urls::encode_path_segment(&format!("{url}:alice"));
    let query = format!("account={expected}");

    let reorder_body = format!(r#"{{"profileIds":["{ID_B}","{ID_A}"]}}"#);
    let reorder = ApiRequest {
        method: "PUT".to_string(),
        query: query.clone(),
        ..post("/api/collections/profiles/order", reorder_body.as_bytes())
    };
    let reorder_resp = send(&fixture, &reorder);
    assert_eq!(reorder_resp.status, 409);
    let body: Value = serde_json::from_slice(&reorder_resp.body).expect("json");
    assert_eq!(body["accountChanged"], true);

    let delete = ApiRequest {
        method: "DELETE".to_string(),
        query: query.clone(),
        ..post(&format!("/api/collections/profiles/{ID_A}"), b"")
    };
    let delete_resp = send(&fixture, &delete);
    assert_eq!(delete_resp.status, 409);
    let body: Value = serde_json::from_slice(&delete_resp.body).expect("json");
    assert_eq!(body["accountChanged"], true);

    let patch = ApiRequest {
        method: "PATCH".to_string(),
        query: query.clone(),
        ..post(
            "/api/collections/settings",
            br#"{"includeUnreleased":true,"modeSelection":"jellyfin"}"#,
        )
    };
    let patch_resp = send(&fixture, &patch);
    assert_eq!(patch_resp.status, 409);
    let body: Value = serde_json::from_slice(&patch_resp.body).expect("json");
    assert_eq!(body["accountChanged"], true);

    let get_settings = ApiRequest {
        method: "GET".to_string(),
        query: query.clone(),
        ..post("/api/collections/settings", b"")
    };
    assert_eq!(send(&fixture, &get_settings).status, 409);

    let get_profiles = ApiRequest {
        method: "GET".to_string(),
        query,
        ..post("/api/collections/profiles", b"")
    };
    assert_eq!(send(&fixture, &get_profiles).status, 409);

    let bob_settings = fixture.services.collections.account(&bob);
    assert_eq!(
        bob_settings
            .profiles
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        vec![ID_A, ID_B]
    );
    assert!(!bob_settings.franchises.include_unreleased);
    assert_ne!(bob_settings.mode_selection, Some(CollectionMode::Jellyfin));
}

#[test]
fn collection_recovery_notice_persists_across_repeated_reads() {
    let dir = std::env::temp_dir().join(format!(
        "mediaflick-collection-recovery-{}",
        crate::app::ids::random_hex(8)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("collections.json");
    crate::preferences::store::atomic_write(&path, b"{not valid json").expect("damaged file");

    let service = CollectionConfigurationService::open(path).expect("recover");
    assert!(service.recovery_notice().is_some());
    assert!(service.recovery_notice().is_some());

    let _ = std::fs::remove_dir_all(&dir);
}
