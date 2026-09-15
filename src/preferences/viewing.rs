use std::io;

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupDestination {
    #[default]
    Home,
    Movies,
    Series,
    Calendar,
    Last,
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
    use super::PlayerComfort;

    #[test]
    fn shortcuts_reject_reserved_duplicate_and_watched_next_keys() {
        let mut comfort = PlayerComfort {
            pause_key: "q".into(),
            ..Default::default()
        };
        assert!(comfort.validate().is_err());
        comfort.pause_key = "m".into();
        assert!(comfort.validate().is_err());
        comfort.pause_key = "p".into();
        assert!(comfort.validate().is_ok());
        assert!(comfort.validate_watched_next(Some("p")).is_err());
        assert!(comfort.validate_watched_next(Some("Ctrl+p")).is_ok());
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

    #[test]
    fn older_settings_keep_their_bindings_and_gain_defaults() -> Result<(), serde_json::Error> {
        let comfort: PlayerComfort = serde_json::from_value(serde_json::json!({"pauseKey":"p"}))?;
        assert_eq!(comfort.pause_key, "p");
        assert_eq!(comfort.stop_key, "q");
        assert_eq!(comfort.seek_back_key, "j");
        assert_eq!(comfort.seek_forward_thirty_key, "UP");
        let saved = serde_json::to_value(&comfort)?;
        assert_eq!(serde_json::from_value::<PlayerComfort>(saved)?, comfort);
        Ok(())
    }
}
