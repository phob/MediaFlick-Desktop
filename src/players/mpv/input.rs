pub const INPUT_SECTION_NAME: &str = "mediaflick_desktop_input";
pub const MARK_WATCHED_NEXT_COMMAND: &str = "mark-watched-next";
const STOP_PLAYBACK_KEYS: &[&str] = &["q", "Q", "CLOSE_WIN", "STOP"];
const SEEK_PLAYBACK_BINDINGS: &[(&str, i32)] =
    &[("LEFT", -10), ("RIGHT", 10), ("DOWN", -30), ("UP", 30)];

/// The mpv input section MediaFlick defines: stop and seek keys, plus the
/// mark-watched-and-play-next binding from Player settings when it is set.
pub fn section_contents(mark_watched_next: Option<&str>) -> String {
    let mut lines = STOP_PLAYBACK_KEYS
        .iter()
        .map(|key| format!("{key} stop"))
        .collect::<Vec<_>>();
    lines.extend(
        SEEK_PLAYBACK_BINDINGS
            .iter()
            .map(|(key, seconds)| format!("{key} seek {seconds} relative+exact")),
    );

    if let Some(key) = mark_watched_next.and_then(sanitize_mpv_key) {
        lines.push(format!(
            "{key} script-message mediaflick-desktop {MARK_WATCHED_NEXT_COMMAND}"
        ));
    }

    lines.join("\n")
}

fn sanitize_mpv_key(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{MARK_WATCHED_NEXT_COMMAND, section_contents};

    #[test]
    fn binds_stop_and_the_watched_key() {
        let section = section_contents(Some("w"));

        for line in [
            "w script-message mediaflick-desktop mark-watched-next",
            "q stop",
            "Q stop",
        ] {
            assert!(section.contains(line), "{line}");
        }
    }

    #[test]
    fn a_disabled_or_unusable_binding_leaves_only_stop_and_seek() {
        for binding in [None, Some(""), Some("two words"), Some("tab\tkey")] {
            let section = section_contents(binding);
            assert!(!section.contains(MARK_WATCHED_NEXT_COMMAND), "{binding:?}");
            assert!(section.contains("q stop"));
        }
    }
}
