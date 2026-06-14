use std::path::PathBuf;

use serde_json::Value;

use crate::app::settings::config_dir;

pub const INPUT_SECTION_NAME: &str = "mediaflick_desktop_input";
pub const MARK_WATCHED_NEXT_COMMAND: &str = "mark-watched-next";
const DEFAULT_MARK_WATCHED_NEXT_KEY: &str = "w";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpvInputBindings {
    pub mark_watched_next: Option<String>,
}

impl Default for MpvInputBindings {
    fn default() -> Self {
        Self {
            mark_watched_next: Some(DEFAULT_MARK_WATCHED_NEXT_KEY.to_string()),
        }
    }
}

impl MpvInputBindings {
    pub fn load() -> Self {
        let path = input_file_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => Self::from_json(&value),
            Err(error) => {
                tracing::warn!("failed to read {}: {error}", path.display());
                Self::default()
            }
        }
    }

    fn from_json(value: &Value) -> Self {
        let mark_watched_next = find_binding(value, "mark_watched_next")
            .or_else(|| find_binding(value, "kb_watched"))
            .unwrap_or_else(|| Some(DEFAULT_MARK_WATCHED_NEXT_KEY.to_string()));
        Self { mark_watched_next }
    }

    pub fn section_contents(&self) -> Option<String> {
        let key = sanitize_mpv_key(self.mark_watched_next.as_deref()?)?;
        Some(format!(
            "{key} script-message mediaflick-desktop {MARK_WATCHED_NEXT_COMMAND}"
        ))
    }
}

pub fn input_file_path() -> PathBuf {
    config_dir().join("input.json")
}

fn find_binding(value: &Value, key: &str) -> Option<Option<String>> {
    value
        .get("bindings")
        .and_then(|bindings| bindings.get(key))
        .or_else(|| value.get(key))
        .map(binding_value)
}

fn binding_value(value: &Value) -> Option<String> {
    value.as_str().and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
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
    use super::{MARK_WATCHED_NEXT_COMMAND, MpvInputBindings};
    use serde_json::json;

    #[test]
    fn defaults_to_watched_skip_on_w() {
        let bindings = MpvInputBindings::from_json(&json!({}));

        assert_eq!(bindings.mark_watched_next.as_deref(), Some("w"));
        assert_eq!(
            bindings.section_contents().as_deref(),
            Some("w script-message mediaflick-desktop mark-watched-next")
        );
    }

    #[test]
    fn reads_nested_binding() {
        let bindings = MpvInputBindings::from_json(&json!({
            "bindings": {
                "mark_watched_next": "W"
            }
        }));

        assert_eq!(bindings.mark_watched_next.as_deref(), Some("W"));
        assert!(
            bindings
                .section_contents()
                .unwrap()
                .contains(MARK_WATCHED_NEXT_COMMAND)
        );
    }

    #[test]
    fn blank_binding_disables_command() {
        let bindings = MpvInputBindings::from_json(&json!({
            "mark_watched_next": ""
        }));

        assert_eq!(bindings.mark_watched_next, None);
        assert_eq!(bindings.section_contents(), None);
    }
}
