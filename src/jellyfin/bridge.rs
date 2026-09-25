//! Shell actions triggered by the native dialogs and startup handshake.
//!
//! The jellyfin-web injection bridge is gone: the own UI talks to Rust over
//! `mediaflick-desktop://app/api/*`. What remains here is the small
//! `mediaflick-desktop://<action>` protocol used by the update toast, plus the
//! readiness fallback used when the typed API is unavailable. A per-session
//! token authenticates every toast action.

use std::sync::OnceLock;

static BRIDGE_TOKEN: OnceLock<String> = OnceLock::new();
const BRIDGE_TOKEN_ENV: &str = "MEDIAFLICK_BRIDGE_TOKEN";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeAction {
    WindowReady,
    DownloadUpdate,
    OpenUpdateRelease,
}

pub fn parse_bridge_action(request_url: &str) -> Option<BridgeAction> {
    let request = request_url.strip_prefix("mediaflick-desktop://")?;
    let request = request.split('#').next().unwrap_or_default();
    let path = request.split('?').next().unwrap_or_default();
    let action = path.strip_suffix('/').unwrap_or(path);
    if action.contains('/') {
        return None;
    }

    Some(match action {
        "window-ready" => BridgeAction::WindowReady,
        "update-download" => BridgeAction::DownloadUpdate,
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
        // SAFETY: main calls this first, before any other thread exists, so no
        // one can read the environment concurrently.
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
    use super::{BridgeAction, parse_bridge_action};

    #[test]
    fn parses_remaining_native_actions_exactly() {
        assert_eq!(
            parse_bridge_action("mediaflick-desktop://update-download?token=x&version=1#ignored"),
            Some(BridgeAction::DownloadUpdate)
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
}
