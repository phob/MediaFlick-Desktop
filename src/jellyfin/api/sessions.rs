//! Session capability announcement.
//!
//! Telling the server this session supports media control is what makes it
//! appear in the "Play On" menus of Jellyfin Web and the mobile apps; without
//! it the server never routes Play, Playstate, or GeneralCommand messages
//! here. Announced once per WebSocket connect, since that is when the server
//! considers the session live.

use serde_json::{Value, json};

use super::client::{ApiError, JellyfinClient};

/// GeneralCommand names this client actually executes. Playstate commands
/// (pause, stop, seek, next) are covered by `SupportsMediaControl` instead.
const SUPPORTED_COMMANDS: [&str; 4] = ["SetVolume", "Mute", "Unmute", "ToggleMute"];

pub fn capabilities_body() -> Value {
    json!({
        "PlayableMediaTypes": ["Video"],
        "SupportedCommands": SUPPORTED_COMMANDS,
        "SupportsMediaControl": true,
        // The session should stop being a cast target once this device
        // disconnects rather than lingering in "Play On" menus.
        "SupportsPersistentIdentifier": false,
    })
}

pub fn announce_capabilities(client: &JellyfinClient) -> Result<(), ApiError> {
    client.post_empty("/Sessions/Capabilities/Full", &[], &capabilities_body())
}

#[cfg(test)]
mod tests {
    use super::capabilities_body;

    #[test]
    fn the_announcement_offers_video_control_without_persistence() {
        let body = capabilities_body();
        assert_eq!(body["PlayableMediaTypes"], serde_json::json!(["Video"]));
        assert_eq!(body["SupportsMediaControl"], true);
        assert_eq!(body["SupportsPersistentIdentifier"], false);
        let commands = body["SupportedCommands"].as_array().expect("commands");
        for command in ["SetVolume", "Mute", "Unmute", "ToggleMute"] {
            assert!(commands.iter().any(|value| value == command));
        }
        // Playstate control is declared through SupportsMediaControl, not here.
        assert!(!commands.iter().any(|value| value == "Play"));
    }
}
