/// Normalize the mpv-compatible subset supported by the built-in shortcut recorder.
pub(super) fn normalize(binding: &str) -> Option<String> {
    let binding = binding.trim();
    if binding.is_empty() {
        return Some(String::new());
    }
    if binding.len() > 80 {
        return None;
    }
    let mut parts = binding.split('+');
    let key = parts.next_back()?;
    let mut modifiers = [false; 4];
    for modifier in parts {
        let index = match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => 0,
            "alt" => 1,
            "shift" => 2,
            "meta" | "super" => 3,
            _ => return None,
        };
        modifiers[index] = true;
    }
    let key = if key.len() == 1 && key.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        modifiers[2] |= key.bytes().any(|byte| byte.is_ascii_uppercase());
        key.to_ascii_lowercase()
    } else {
        let name = key.to_ascii_uppercase();
        if ![
            "SPACE", "ENTER", "TAB", "ESC", "BS", "DEL", "INS", "HOME", "END", "PGUP", "PGDWN",
            "UP", "DOWN", "LEFT", "RIGHT",
        ]
        .contains(&name.as_str())
            && !name
                .strip_prefix('F')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=24).contains(&n) && name == format!("F{n}"))
        {
            return None;
        }
        name
    };
    let mut parts: Vec<&str> = ["Ctrl", "Alt", "Shift", "Meta"]
        .into_iter()
        .zip(modifiers)
        .filter_map(|(name, enabled)| enabled.then_some(name))
        .collect();
    parts.push(&key);
    Some(parts.join("+"))
}

pub(super) fn reserved(binding: &str) -> bool {
    binding.rsplit('+').next() == Some("F11")
        || ["SPACE", "LEFT", "RIGHT", "ESC", "TAB", "Alt+F4"].contains(&binding)
}

#[cfg(test)]
mod tests {
    #[test]
    fn shortcut_contract_matches_the_ui() -> Result<(), Box<dyn std::error::Error>> {
        #[derive(serde::Deserialize)]
        struct Case {
            input: String,
            normalized: Option<String>,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../ui/test/fixtures/player-shortcuts.json"))?;
        for case in cases {
            assert_eq!(
                super::normalize(&case.input),
                case.normalized,
                "{}",
                case.input
            );
        }
        Ok(())
    }
}
