use super::AppSettings;

/// Runtime effects required after applying a preference change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsApplyPlan {
    pub rebuild_player: bool,
    pub update_segment_policy: bool,
    pub update_shell_css: bool,
    pub restart_required: bool,
}

impl SettingsApplyPlan {
    pub fn between(previous: &AppSettings, next: &AppSettings) -> Self {
        Self {
            rebuild_player: previous.effective_backend() != next.effective_backend()
                || match next.effective_backend() {
                    super::PlayerBackend::Mpv => previous.mpv_path != next.mpv_path,
                    super::PlayerBackend::Mpchc => previous.mpchc_path != next.mpchc_path,
                },
            update_segment_policy: previous.segment_skip_config() != next.segment_skip_config(),
            update_shell_css: previous.show_scrollbars != next.show_scrollbars,
            restart_required: previous.log_level != next.log_level,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::{SegmentSkipMode, StreamingQuality};

    #[test]
    fn ignores_inactive_paths_and_non_destructive_window_defaults() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.mpchc_path = Some("other.exe".to_string());
        next.default_fullscreen = crate::preferences::FullscreenBehavior::Windowed;

        assert!(!SettingsApplyPlan::between(&previous, &next).rebuild_player);
    }

    #[test]
    fn reports_only_the_runtime_effects_required_by_a_change() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.mpv_path = Some("other-mpv".to_string());
        next.streaming_quality = StreamingQuality::Auto;
        next.skip_intro = SegmentSkipMode::Always;
        next.show_scrollbars = !previous.show_scrollbars;
        next.log_level = "trace".to_string();

        assert_eq!(
            SettingsApplyPlan::between(&previous, &next),
            SettingsApplyPlan {
                rebuild_player: true,
                update_segment_policy: true,
                update_shell_css: true,
                restart_required: true,
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn switching_the_effective_backend_rebuilds_the_player() {
        let previous = AppSettings::default();
        let mut next = previous.clone();
        next.player_backend = crate::preferences::PlayerBackend::Mpchc;

        let plan = SettingsApplyPlan::between(&previous, &next);
        assert!(plan.rebuild_player);
        assert!(!plan.update_segment_policy);
        assert!(!plan.update_shell_css);
        assert!(!plan.restart_required);
    }
}
