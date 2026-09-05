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
        }
    }
}

impl PlayerComfort {
    pub fn validate_watched_next(&self, binding: Option<&str>) -> io::Result<()> {
        if binding.is_some_and(|binding| {
            [&self.pause_key, &self.mute_key, &self.fullscreen_key]
                .iter()
                .any(|key| key.as_str() == binding.trim())
        }) {
            return Err(io::Error::other(
                "player shortcut conflicts with the mark-watched-next key",
            ));
        }
        Ok(())
    }

    pub fn validate(&self) -> io::Result<()> {
        let keys = [&self.pause_key, &self.mute_key, &self.fullscreen_key];
        if !(50..=200).contains(&self.subtitle_size)
            || self.subtitle_outline > 8
            || self.subtitle_background > 100
            || self.subtitle_position > 100
            || !(1..=120).contains(&self.seek_back_seconds)
            || !(1..=120).contains(&self.seek_forward_seconds)
            || keys.iter().any(|key| {
                key.len() != 1
                    || !key.bytes().all(|b| b.is_ascii_lowercase())
                    || ["j", "l", "q", "v"].contains(&key.as_str())
            })
            || keys[0] == keys[1]
            || keys[0] == keys[2]
            || keys[1] == keys[2]
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
}
