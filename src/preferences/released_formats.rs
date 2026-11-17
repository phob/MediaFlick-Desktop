//! The JSON files a released MediaFlick wrote still load, keep their values,
//! and survive a save.
//!
//! The files in `fixtures/preferences/released/0.2.0/` are what the released
//! 0.2.0 AppImage wrote while a drive set each choice through its UI
//! (sanitized by `scripts/fixtures/freeze_0_2_0.py`). The expected values are
//! the choices that drive recorded in its `fixture-intent.json`: Default
//! fullscreen Windowed, Show scrollbars on, Hide watched by default, audio
//! languages en and de, the Cobalt accent with IMDb and Rotten Tomatoes
//! Critics ratings, one Letterboxd profile, a daily "Trending today"
//! collection with a custom poster, and an English SRT subtitle on a movie.
//!
//! Failure modes: a released file fails to load or is moved aside as
//! damaged; it loads but a value changes meaning or falls back to its
//! default; a save through the store drops or rewrites a value it did not
//! change. Not covered: `reader.json` and `pending-deletions.json`, which the
//! 0.2.0 profile has no copy of.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::json_file::load_with_recovery;
use super::{
    AccountConfigurationService, AccountKey, AppSettings, AppearanceAccent,
    CollectionConfigurationService, FullscreenBehavior, PlaybackPreferenceService,
};
use crate::collections::model::RefreshCadence;

/// The sanitized server and user of the 0.2.0 profile.
const SERVER: &str = "f1c70000000000000000000000000001";
const USER: &str = "f1c70000000000000000000000000002";

struct Released {
    dir: PathBuf,
}

impl Released {
    /// A scratch copy of the released 0.2.0 files.
    fn copy() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mediaflick-released-{}",
            crate::app::ids::random_hex(8)
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/preferences/released/0.2.0");
        for entry in std::fs::read_dir(source).expect("released 0.2.0 files") {
            let entry = entry.expect("entry");
            std::fs::copy(entry.path(), dir.join(entry.file_name())).expect("copy");
        }
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn json(&self, name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.path(name)).expect("read")).expect("JSON")
    }

    /// Opening must not have moved anything aside or written anything.
    fn assert_untouched(&self, name: &str) {
        let original = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/preferences/released/0.2.0")
                .join(name),
        )
        .expect("fixture");
        assert_eq!(
            std::fs::read(self.path(name)).expect("read"),
            original,
            "opening {name} rewrote it"
        );
        let names = std::fs::read_dir(&self.dir)
            .expect("scratch")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
            .filter(|file| file.starts_with(name) && file != name)
            .collect::<Vec<_>>();
        assert!(names.is_empty(), "opening {name} left {names:?}");
    }
}

impl Drop for Released {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn key() -> AccountKey {
    AccountKey::new(SERVER, USER).expect("account key")
}

#[test]
fn settings_json_from_0_2_0_loads_its_values() {
    let released = Released::copy();
    let loaded = load_with_recovery::<AppSettings>(&released.path("settings.json"))
        .expect("settings.json loads")
        .expect("settings.json exists");
    assert!(loaded.recovery.is_none());
    released.assert_untouched("settings.json");
    let settings = loaded.document;
    assert_eq!(settings.default_fullscreen, FullscreenBehavior::Windowed);
    assert!(settings.show_scrollbars);
    assert_eq!(
        serde_json::to_value(&settings).expect("serialize"),
        released.json("settings.json"),
        "writing the loaded settings back drops or changes a value"
    );
}

#[test]
fn accounts_json_from_0_2_0_loads_and_survives_a_save() {
    let released = Released::copy();
    let before = released.json("accounts.json");
    let accounts =
        AccountConfigurationService::open(released.path("accounts.json")).expect("opens");
    assert!(accounts.take_recovery_notice().is_none());
    released.assert_untouched("accounts.json");

    let viewing = accounts.viewing(&key());
    assert!(viewing.hide_watched);
    assert_eq!(viewing.audio_languages, ["en", "de"]);
    let appearance = accounts.appearance(&key());
    assert_eq!(appearance.accent, AppearanceAccent::Cobalt);
    assert_eq!(appearance.rating_sources, ["imdb", "tomatoes"]);
    let letterboxd = accounts.letterboxd_profiles(&key());
    assert_eq!(
        letterboxd
            .iter()
            .map(|profile| (profile.profile_key.as_str(), profile.enabled))
            .collect::<Vec<_>>(),
        [("fixture-critic", true)]
    );

    accounts
        .save_viewing_with(&key(), Clone::clone)
        .expect("save");
    assert_eq!(
        released.json("accounts.json"),
        before,
        "a save that changes nothing rewrote a value"
    );
    let reopened =
        AccountConfigurationService::open(released.path("accounts.json")).expect("reopens");
    assert_eq!(reopened.viewing(&key()), viewing);
    assert_eq!(reopened.appearance(&key()), appearance);
}

#[test]
fn collections_json_from_0_2_0_loads_and_survives_a_save() {
    let released = Released::copy();
    let before = released.json("collections.json");
    let collections =
        CollectionConfigurationService::open(released.path("collections.json")).expect("opens");
    assert!(collections.take_recovery_notice().is_none());
    released.assert_untouched("collections.json");

    let account = collections.account(&key());
    let profiles = account
        .profiles
        .iter()
        .map(|profile| {
            (
                profile.title.as_str(),
                profile.template.id.as_str(),
                profile.cadence,
                profile.custom_poster_id.is_some(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        profiles,
        [(
            "Trending today",
            "tmdb.discover.movie.trending-day",
            RefreshCadence::Daily,
            true
        )]
    );
    assert_eq!(
        collections.artwork_ids_for_account(&key()),
        account
            .profiles
            .iter()
            .filter_map(|profile| profile.custom_poster_id.clone())
            .collect::<Vec<_>>()
    );

    collections
        .set_include_unreleased(&key(), account.franchises.include_unreleased)
        .expect("save");
    assert_eq!(
        released.json("collections.json"),
        before,
        "a save that changes nothing rewrote a value"
    );
}

#[test]
fn playback_preferences_json_from_0_2_0_loads_and_survives_a_save() {
    let released = Released::copy();
    let before = released.json("playback-preferences.json");
    let item = before["accounts"][0]["items"][0]["itemId"]
        .as_str()
        .expect("item id")
        .to_owned();
    let playback = PlaybackPreferenceService::open(released.path("playback-preferences.json"))
        .expect("opens");
    assert!(playback.take_recovery_notice().is_none());
    released.assert_untouched("playback-preferences.json");

    let preference = playback.get(&key(), &item).expect("the saved choice");
    let subtitle = preference.subtitle_track.clone().expect("a subtitle");
    assert_eq!(
        (
            subtitle.index,
            subtitle.language.as_deref(),
            subtitle.codec.as_deref()
        ),
        (4, Some("eng"), Some("subrip"))
    );

    playback.save(&key(), &item, &preference).expect("save");
    let without_time = |mut document: Value| {
        if let Some(entry) = document["accounts"][0]["items"][0].as_object_mut() {
            entry.remove("updatedAt");
        }
        document
    };
    assert_eq!(
        without_time(released.json("playback-preferences.json")),
        without_time(before),
        "saving the same choice rewrote a value"
    );
}
