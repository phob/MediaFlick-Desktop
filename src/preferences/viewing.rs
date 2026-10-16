use std::io;

use serde::{Deserialize, Serialize};

use crate::library::libraries::{LibraryId, LibraryKind, LibraryRegistry};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NextEpisode {
    Off,
    Ask,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleMode {
    #[default]
    Server,
    Off,
    Forced,
    Always,
    ForeignAudio,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupDestination {
    #[default]
    Home,
    /// Legacy: read as the first Movies library and never written by this version.
    Movies,
    /// Legacy: read as the first TV shows library and never written by this version.
    Series,
    Calendar,
    Last,
    Library(LibraryId),
}

/// The destination the UI acts on. While the registry is unknown, a legacy
/// value reads as Home and a library passes through.
pub fn startup_view(stored: &StartupDestination, registry: &LibraryRegistry) -> StartupDestination {
    let LibraryRegistry::Known(libraries) = registry else {
        return if matches!(
            stored,
            StartupDestination::Movies | StartupDestination::Series
        ) {
            StartupDestination::Home
        } else {
            stored.clone()
        };
    };
    let first_of = |kind| {
        libraries
            .iter()
            .find(|library| library.kind() == kind)
            .map_or(StartupDestination::Home, |library| {
                StartupDestination::Library(library.id.clone())
            })
    };
    match stored {
        StartupDestination::Movies => first_of(LibraryKind::Movies),
        StartupDestination::Series => first_of(LibraryKind::Shows),
        StartupDestination::Library(id) if !libraries.iter().any(|library| library.id == *id) => {
            StartupDestination::Home
        }
        _ => stored.clone(),
    }
}

/// The destination a save stores. Callers send back what `startup_view`
/// showed them, so that value keeps the stored one and a save of another
/// setting cannot resolve a legacy choice or a missing library away.
pub fn startup_on_save(
    stored: &StartupDestination,
    submitted: StartupDestination,
    registry: &LibraryRegistry,
) -> io::Result<StartupDestination> {
    if matches!(
        submitted,
        StartupDestination::Movies | StartupDestination::Series
    ) {
        return Err(io::Error::other(
            "choose a library instead of Movies or Series as the startup destination",
        ));
    }
    Ok(if submitted == startup_view(stored, registry) {
        stored.clone()
    } else {
        submitted
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewingSettings {
    pub spoiler_protection: bool,
    pub next_episode: NextEpisode,
    pub countdown_seconds: u8,
    /// Zero leaves continuous playback unlimited.
    pub episode_limit: u8,
    pub audio_languages: Vec<String>,
    pub subtitle_languages: Vec<String>,
    pub prefer_original_audio: bool,
    pub subtitle_mode: SubtitleMode,
    pub resume_rewind_seconds: u8,
    pub text_scale: u16,
    pub poster_size: u16,
    pub preview_delay_ms: u16,
    pub startup_destination: StartupDestination,
    pub remember_filters: bool,
    pub hide_watched: bool,
}

impl Default for ViewingSettings {
    fn default() -> Self {
        Self {
            spoiler_protection: false,
            next_episode: NextEpisode::Auto,
            countdown_seconds: 10,
            episode_limit: 0,
            audio_languages: Vec::new(),
            subtitle_languages: Vec::new(),
            prefer_original_audio: false,
            subtitle_mode: SubtitleMode::Server,
            resume_rewind_seconds: 0,
            text_scale: 100,
            poster_size: 168,
            preview_delay_ms: 550,
            startup_destination: StartupDestination::Home,
            remember_filters: false,
            hide_watched: false,
        }
    }
}

impl ViewingSettings {
    pub fn validate(&self) -> io::Result<()> {
        if self.subtitle_mode == SubtitleMode::ForeignAudio && self.audio_languages.is_empty() {
            return Err(io::Error::other(
                "choose a preferred audio language for conditional subtitles",
            ));
        }
        if !(3..=60).contains(&self.countdown_seconds)
            || self.episode_limit > 20
            || ![0, 5, 10, 30].contains(&self.resume_rewind_seconds)
            || !(80..=150).contains(&self.text_scale)
            || !(120..=240).contains(&self.poster_size)
            || !(200..=2000).contains(&self.preview_delay_ms)
        {
            return Err(io::Error::other(
                "viewing setting is outside its supported range",
            ));
        }
        for languages in [&self.audio_languages, &self.subtitle_languages] {
            if languages.len() > 8
                || languages.iter().any(|language| {
                    !(2..=12).contains(&language.len())
                        || !language
                            .bytes()
                            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
                })
            {
                return Err(io::Error::other(
                    "use up to eight language codes, such as en, eng, or de",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerComfort {
    pub subtitle_size: u16,
    pub subtitle_outline: u8,
    pub subtitle_background: u8,
    pub subtitle_position: u8,
    pub seek_back_seconds: u8,
    pub seek_forward_seconds: u8,
    pub pause_key: String,
    pub mute_key: String,
    pub fullscreen_key: String,
    pub seek_back_key: String,
    pub seek_forward_key: String,
    pub stop_key: String,
    pub subtitles_key: String,
    pub seek_back_thirty_key: String,
    pub seek_forward_thirty_key: String,
}

impl Default for PlayerComfort {
    fn default() -> Self {
        Self {
            subtitle_size: 100,
            subtitle_outline: 3,
            subtitle_background: 0,
            subtitle_position: 100,
            seek_back_seconds: 10,
            seek_forward_seconds: 30,
            pause_key: "k".into(),
            mute_key: "m".into(),
            fullscreen_key: "f".into(),
            seek_back_key: "j".into(),
            seek_forward_key: "l".into(),
            stop_key: "q".into(),
            subtitles_key: "v".into(),
            seek_back_thirty_key: "DOWN".into(),
            seek_forward_thirty_key: "UP".into(),
        }
    }
}

impl PlayerComfort {
    fn shortcut_keys(&self) -> [&str; 9] {
        [
            &self.pause_key,
            &self.mute_key,
            &self.fullscreen_key,
            &self.seek_back_key,
            &self.seek_forward_key,
            &self.stop_key,
            &self.subtitles_key,
            &self.seek_back_thirty_key,
            &self.seek_forward_thirty_key,
        ]
    }

    pub fn validate_watched_next(&self, binding: Option<&str>) -> io::Result<()> {
        let Some(binding) = binding.filter(|binding| !binding.trim().is_empty()) else {
            return Ok(());
        };
        let normalized = super::shortcuts::normalize(binding)
            .ok_or_else(|| io::Error::other("unsupported built-in player shortcut"))?;
        if super::shortcuts::reserved(&normalized)
            || self
                .shortcut_keys()
                .iter()
                .any(|key| super::shortcuts::normalize(key).as_ref() == Some(&normalized))
        {
            return Err(io::Error::other(
                "player shortcut conflicts with the mark-watched-next key",
            ));
        }
        Ok(())
    }

    pub fn validate(&self) -> io::Result<()> {
        let keys = self.shortcut_keys().map(super::shortcuts::normalize);
        if !(50..=200).contains(&self.subtitle_size)
            || self.subtitle_outline > 8
            || self.subtitle_background > 100
            || self.subtitle_position > 100
            || !(1..=120).contains(&self.seek_back_seconds)
            || !(1..=120).contains(&self.seek_forward_seconds)
            || keys
                .iter()
                .any(|key| key.as_deref().is_none_or(super::shortcuts::reserved))
            || keys
                .iter()
                .enumerate()
                .any(|(index, key)| key.as_deref() != Some("") && keys[..index].contains(key))
        {
            return Err(io::Error::other(
                "invalid player comfort settings or conflicting shortcut keys",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PlayerComfort, StartupDestination, ViewingSettings, startup_on_save, startup_view,
    };
    use crate::jellyfin::api::model::UserViewDto;
    use crate::library::libraries::{LibraryId, LibraryRegistry, MediaLibrary};

    const MOVIES_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const MOVIES_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHOWS: &str = "cccccccccccccccccccccccccccccccc";
    const MIXED: &str = "dddddddddddddddddddddddddddddddd";
    const GONE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    fn known(views: &[(&str, Option<&str>)]) -> LibraryRegistry {
        LibraryRegistry::Known(
            views
                .iter()
                .filter_map(|(id, collection_type)| {
                    MediaLibrary::from_view(&UserViewDto {
                        id: (*id).into(),
                        collection_type: collection_type.map(str::to_string),
                        ..Default::default()
                    })
                })
                .collect(),
        )
    }

    fn library(id: &str) -> StartupDestination {
        StartupDestination::Library(LibraryId::parse(id).expect("test id"))
    }

    #[test]
    fn legacy_destinations_open_the_first_library_of_their_kind_in_server_order() {
        let registry = known(&[
            (MIXED, None),
            (SHOWS, Some("tvshows")),
            (MOVIES_A, Some("movies")),
            (MOVIES_B, Some("movies")),
        ]);
        assert_eq!(
            startup_view(&StartupDestination::Movies, &registry),
            library(MOVIES_A)
        );
        assert_eq!(
            startup_view(&StartupDestination::Series, &registry),
            library(SHOWS)
        );
    }

    #[test]
    fn legacy_destinations_open_home_without_a_library_of_their_kind_or_a_known_list() {
        let shows_only = known(&[(MIXED, None), (SHOWS, Some("tvshows"))]);
        assert_eq!(
            startup_view(&StartupDestination::Movies, &shows_only),
            StartupDestination::Home
        );
        assert_eq!(
            startup_view(&StartupDestination::Series, &known(&[])),
            StartupDestination::Home
        );
        for legacy in [StartupDestination::Movies, StartupDestination::Series] {
            assert_eq!(
                startup_view(&legacy, &LibraryRegistry::Unknown),
                StartupDestination::Home
            );
        }
    }

    #[test]
    fn a_library_destination_opens_home_only_when_a_known_list_lacks_it() {
        let registry = known(&[(SHOWS, Some("tvshows"))]);
        assert_eq!(startup_view(&library(SHOWS), &registry), library(SHOWS));
        assert_eq!(
            startup_view(&library(GONE), &registry),
            StartupDestination::Home
        );
        assert_eq!(
            startup_view(&library(GONE), &LibraryRegistry::Unknown),
            library(GONE)
        );
    }

    #[test]
    fn other_destinations_pass_through() {
        for registry in [LibraryRegistry::Unknown, known(&[(SHOWS, Some("tvshows"))])] {
            for destination in [
                StartupDestination::Home,
                StartupDestination::Calendar,
                StartupDestination::Last,
            ] {
                assert_eq!(startup_view(&destination, &registry), destination);
            }
        }
    }

    #[test]
    fn saving_the_shown_destination_keeps_the_stored_one() -> std::io::Result<()> {
        let movies_and_shows = known(&[(MOVIES_A, Some("movies")), (SHOWS, Some("tvshows"))]);
        let cases = [
            (
                StartupDestination::Movies,
                StartupDestination::Home,
                LibraryRegistry::Unknown,
            ),
            (
                StartupDestination::Movies,
                library(MOVIES_A),
                movies_and_shows.clone(),
            ),
            (
                StartupDestination::Series,
                StartupDestination::Home,
                known(&[(MOVIES_A, Some("movies"))]),
            ),
            (library(GONE), StartupDestination::Home, movies_and_shows),
        ];
        for (stored, shown, registry) in cases {
            assert_eq!(startup_on_save(&stored, shown, &registry)?, stored);
        }
        Ok(())
    }

    #[test]
    fn saving_a_different_destination_stores_it_as_submitted() -> std::io::Result<()> {
        let registry = known(&[(MOVIES_A, Some("movies")), (SHOWS, Some("tvshows"))]);
        let cases = [
            (
                StartupDestination::Home,
                library(GONE),
                LibraryRegistry::Unknown,
            ),
            (StartupDestination::Home, library(GONE), registry.clone()),
            (
                StartupDestination::Movies,
                StartupDestination::Home,
                registry.clone(),
            ),
            (
                StartupDestination::Movies,
                StartupDestination::Calendar,
                LibraryRegistry::Unknown,
            ),
            (
                StartupDestination::Series,
                StartupDestination::Last,
                registry.clone(),
            ),
            (library(MOVIES_A), library(SHOWS), registry),
        ];
        for (stored, submitted, registry) in cases {
            assert_eq!(
                startup_on_save(&stored, submitted.clone(), &registry)?,
                submitted
            );
        }
        Ok(())
    }

    #[test]
    fn saving_a_legacy_destination_is_rejected() {
        let registry = known(&[(MOVIES_A, Some("movies")), (SHOWS, Some("tvshows"))]);
        for registry in [LibraryRegistry::Unknown, registry] {
            for stored in [
                StartupDestination::Home,
                StartupDestination::Movies,
                StartupDestination::Series,
            ] {
                for legacy in [StartupDestination::Movies, StartupDestination::Series] {
                    assert!(startup_on_save(&stored, legacy, &registry).is_err());
                }
            }
        }
    }

    #[test]
    fn startup_destination_wire_forms() -> Result<(), serde_json::Error> {
        let parse = |destination: &str| {
            serde_json::from_str::<ViewingSettings>(&format!(
                r#"{{"startupDestination":{destination}}}"#
            ))
            .map(|settings| settings.startup_destination)
        };
        assert_eq!(parse(r#""movies""#)?, StartupDestination::Movies);
        assert_eq!(parse(r#""series""#)?, StartupDestination::Series);
        assert_eq!(
            parse(&format!(r#"{{"library":"{SHOWS}"}}"#))?,
            library(SHOWS)
        );
        assert!(parse(&format!(r#"{{"library":"{}"}}"#, SHOWS.to_uppercase())).is_err());
        assert!(parse(&format!(r#"{{"library":" {SHOWS}"}}"#)).is_err());
        assert!(parse(r#"{"library":"../etc"}"#).is_err());
        assert!(parse(r#"{"library":""}"#).is_err());
        assert_eq!(
            serde_json::to_value(library(SHOWS))?,
            serde_json::json!({ "library": SHOWS })
        );
        Ok(())
    }

    #[test]
    fn combinations_allow_disabling_and_reject_equivalent_conflicts() {
        let mut comfort = PlayerComfort {
            pause_key: "Ctrl+Shift+p".into(),
            mute_key: String::new(),
            stop_key: String::new(),
            ..Default::default()
        };
        assert!(comfort.validate().is_ok());
        assert!(
            comfort
                .validate_watched_next(Some("shift+control+p"))
                .is_err()
        );
        assert!(comfort.validate_watched_next(Some("Meta+p")).is_ok());
        comfort.stop_key = "Ctrl+P".into();
        assert!(comfort.validate().is_err());
        comfort.stop_key = "SPACE".into();
        assert!(comfort.validate().is_err());
    }
}
