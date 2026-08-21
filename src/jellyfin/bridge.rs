//! Shell actions triggered by the native dialogs and startup handshake.
//!
//! The jellyfin-web injection bridge is gone: the own UI talks to Rust over
//! `mediaflick-desktop://app/api/*`. What remains here is the small
//! `mediaflick-desktop://<action>` protocol used by the About and update-toast
//! dialogs, plus the readiness fallback used when the typed API is unavailable.
//! A per-session token authenticates every dialog action.

use std::sync::OnceLock;

static BRIDGE_TOKEN: OnceLock<String> = OnceLock::new();
const BRIDGE_TOKEN_ENV: &str = "MEDIAFLICK_BRIDGE_TOKEN";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeAction<'a> {
    About,
    Exit,
    WindowReady,
    DownloadUpdate(&'a str),
    OpenUpdateRelease,
}

pub fn parse_bridge_action(request_url: &str) -> Option<BridgeAction<'_>> {
    let request = request_url.strip_prefix("mediaflick-desktop://")?;
    let request = request.split('#').next().unwrap_or_default();
    let (path, query) = request
        .split_once('?')
        .map_or((request, ""), |(path, query)| (path, query));
    let action = path.strip_suffix('/').unwrap_or(path);
    if action.contains('/') {
        return None;
    }

    Some(match action {
        "app-about" => BridgeAction::About,
        "app-exit" => BridgeAction::Exit,
        "window-ready" => BridgeAction::WindowReady,
        "update-download" => BridgeAction::DownloadUpdate(query),
        "update-release" => BridgeAction::OpenUpdateRelease,
        _ => return None,
    })
}

/// Generates the session token once in the browser process and hands it to CEF
/// subprocesses through the environment.
pub fn ensure_session_token() {
    BRIDGE_TOKEN.get_or_init(|| {
        if let Ok(token) = std::env::var(BRIDGE_TOKEN_ENV)
            && !token.is_empty()
        {
            return token;
        }
        let token = crate::app::ids::random_hex(32);
        unsafe {
            std::env::set_var(BRIDGE_TOKEN_ENV, &token);
        }
        token
    });
}

pub fn bridge_token() -> &'static str {
    BRIDGE_TOKEN.get_or_init(|| {
        std::env::var(BRIDGE_TOKEN_ENV)
            .ok()
            .filter(|token| !token.is_empty())
            .unwrap_or_else(|| crate::app::ids::random_hex(32))
    })
}

#[cfg(test)]
mod tests {
    use super::{BridgeAction, bridge_token, parse_bridge_action};

    #[test]
    fn parses_remaining_native_actions_exactly() {
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://update-download?token=x&version=1#ignored"),
            Some(BridgeAction::DownloadUpdate("token=x&version=1"))
        );
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://window-ready"),
            Some(BridgeAction::WindowReady)
        );
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://client-settings/"),
            None
        );
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://app-exit-malicious"),
            None
        );
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://client-settings/extra"),
            None
        );
    }

    #[test]
    fn the_removed_playback_actions_are_no_longer_routable() {
        for url in [
            "mediaflick-desktop://play?payload=%7B%7D",
            "mediaflick-desktop://play-context?payload=%7B%7D",
            "mediaflick-desktop://player-command?payload=%7B%7D",
            "mediaflick-desktop://player-state?requestId=1",
            "mediaflick-desktop://playback-stop-ack?payload=%7B%7D",
            "mediaflick-desktop://save?server=http%3A%2F%2Fhost",
        ] {
            assert_eq!(parse_bridge_action(url), None, "url {url}");
        }
    }

    #[test]
    fn the_app_host_is_not_a_dialog_action() {
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://app/api/home"),
            None
        );
    }

    #[test]
    fn session_bridge_token_is_stable_and_hex() {
        let token = bridge_token();
        assert_eq!(token, bridge_token());
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}
