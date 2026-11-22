use serde_json::{Value, json};

use super::client::{ApiError, JellyfinClient};

const SUPPORTED_COMMANDS: [&str; 4] = ["SetVolume", "Mute", "Unmute", "ToggleMute"];

pub fn capabilities_body() -> Value {
    json!({
        "PlayableMediaTypes": ["Video", "Audio"],
        "SupportedCommands": SUPPORTED_COMMANDS,
        "SupportsMediaControl": true,
        "SupportsPersistentIdentifier": false,
    })
}

pub fn announce_capabilities(client: &JellyfinClient) -> Result<(), ApiError> {
    client.post_empty("/Sessions/Capabilities/Full", &[], &capabilities_body())
}
