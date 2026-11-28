use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use zbus::blocking::Connection;
use zbus::fdo;
use zbus::message::{Header, Message};
use zbus::names::{InterfaceName, MemberName};
use zbus::object_server::{DispatchResult2, Interface, ObjectServer, SignalEmitter};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::{OsMediaAction, OsMediaMetadata, OsMediaState, OsPlaybackStatus};

const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const BUS_PREFIX: &str = "org.mpris.MediaPlayer2.mediaflick_desktop";

type ActionHandler = Arc<dyn Fn(OsMediaAction) + Send + Sync>;

enum WorkerMsg {
    Update(OsMediaState),
    Stop,
}

pub(super) struct LinuxOsMedia {
    tx: Sender<WorkerMsg>,
    thread: Option<JoinHandle<()>>,
}

impl LinuxOsMedia {
    pub(super) fn start(handler: ActionHandler) -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("os-media-mpris".to_string())
            .spawn(move || run_worker(&rx, handler))
            .ok();
        Self { tx, thread }
    }

    pub(super) fn update(&self, state: &OsMediaState) {
        let _ = self.tx.send(WorkerMsg::Update(state.clone()));
    }
}

impl Drop for LinuxOsMedia {
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerMsg::Stop);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

struct SharedPlayerState {
    media: OsMediaState,
    updated_at: Instant,
}

impl Default for SharedPlayerState {
    fn default() -> Self {
        Self {
            media: OsMediaState {
                status: OsPlaybackStatus::Stopped,
                position_us: 0,
                duration_us: 0,
                volume: 1.0,
                can_seek: false,
                metadata: None,
            },
            updated_at: Instant::now(),
        }
    }
}

impl SharedPlayerState {
    fn extrapolated_position_us(&self) -> i64 {
        let base = self.media.position_us.max(0);
        if self.media.status != OsPlaybackStatus::Playing {
            return base;
        }
        let elapsed = i64::try_from(self.updated_at.elapsed().as_micros()).unwrap_or(0);
        let pos = base.saturating_add(elapsed);
        if self.media.duration_us > 0 {
            pos.min(self.media.duration_us)
        } else {
            pos
        }
    }
}

struct MprisRoot {
    handler: ActionHandler,
}

#[zbus::export::async_trait::async_trait]
impl Interface for MprisRoot {
    fn name() -> InterfaceName<'static> {
        InterfaceName::from_static_str_unchecked(ROOT_IFACE)
    }

    async fn get(
        &self,
        property_name: &str,
        _server: &ObjectServer,
        _connection: &zbus::Connection,
        _header: Option<&Header<'_>>,
        _emitter: &SignalEmitter<'_>,
    ) -> Option<fdo::Result<OwnedValue>> {
        root_property(property_name).map(Ok)
    }

    async fn get_all(
        &self,
        _object_server: &ObjectServer,
        _connection: &zbus::Connection,
        _header: Option<&Header<'_>>,
        _emitter: &SignalEmitter<'_>,
    ) -> fdo::Result<HashMap<String, OwnedValue>> {
        let mut map = HashMap::new();
        for key in [
            "Identity",
            "DesktopEntry",
            "CanQuit",
            "CanRaise",
            "CanSetFullscreen",
            "HasTrackList",
            "SupportedUriSchemes",
            "SupportedMimeTypes",
        ] {
            if let Some(val) = root_property(key) {
                map.insert(key.to_string(), val);
            }
        }
        Ok(map)
    }

    async fn set_mut(
        &mut self,
        _property_name: &str,
        _value: &Value<'_>,
        _object_server: &ObjectServer,
        _connection: &zbus::Connection,
        _header: Option<&Header<'_>>,
        _emitter: &SignalEmitter<'_>,
    ) -> Option<fdo::Result<()>> {
        None
    }

    fn call<'call>(
        &'call self,
        _server: &'call ObjectServer,
        connection: &'call zbus::Connection,
        msg: &'call Message,
        name: MemberName<'call>,
    ) -> DispatchResult2<'call> {
        let handler = Arc::clone(&self.handler);
        match name.as_str() {
            "Raise" => DispatchResult2::new_async(connection, msg, async move {
                handler(OsMediaAction::Raise);
                Ok::<(), zbus::fdo::Error>(())
            }),
            "Quit" => DispatchResult2::new_async(connection, msg, async {
                Ok::<(), zbus::fdo::Error>(())
            }),
            _ => DispatchResult2::NotFound,
        }
    }

    fn call_mut<'call>(
        &'call mut self,
        _server: &'call ObjectServer,
        _connection: &'call zbus::Connection,
        _msg: &'call Message,
        _name: MemberName<'call>,
    ) -> DispatchResult2<'call> {
        DispatchResult2::NotFound
    }

    fn introspect_to_writer(&self, writer: &mut dyn FmtWrite, _level: usize) {
        let _ = writer.write_str(ROOT_INTROSPECT);
    }
}

struct MprisPlayer {
    shared: Arc<Mutex<SharedPlayerState>>,
    handler: ActionHandler,
}

#[zbus::export::async_trait::async_trait]
impl Interface for MprisPlayer {
    fn name() -> InterfaceName<'static> {
        InterfaceName::from_static_str_unchecked(PLAYER_IFACE)
    }

    async fn get(
        &self,
        property_name: &str,
        _server: &ObjectServer,
        _connection: &zbus::Connection,
        _header: Option<&Header<'_>>,
        _emitter: &SignalEmitter<'_>,
    ) -> Option<fdo::Result<OwnedValue>> {
        let guard = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        player_property(&guard, property_name).map(Ok)
    }

    async fn get_all(
        &self,
        _object_server: &ObjectServer,
        _connection: &zbus::Connection,
        _header: Option<&Header<'_>>,
        _emitter: &SignalEmitter<'_>,
    ) -> fdo::Result<HashMap<String, OwnedValue>> {
        let guard = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(player_properties_map(&guard))
    }

    async fn set_mut(
        &mut self,
        property_name: &str,
        value: &Value<'_>,
        _object_server: &ObjectServer,
        _connection: &zbus::Connection,
        _header: Option<&Header<'_>>,
        _emitter: &SignalEmitter<'_>,
    ) -> Option<fdo::Result<()>> {
        match property_name {
            "Volume" => {
                if let Ok(volume) = f64::try_from(value) {
                    (self.handler)(OsMediaAction::SetVolume(volume.clamp(0.0, 1.0)));
                }
                Some(Ok(()))
            }
            "LoopStatus" | "Rate" | "Shuffle" => Some(Ok(())),
            _ => None,
        }
    }

    fn call<'call>(
        &'call self,
        _server: &'call ObjectServer,
        connection: &'call zbus::Connection,
        msg: &'call Message,
        name: MemberName<'call>,
    ) -> DispatchResult2<'call> {
        dispatch_player_call(&self.shared, &self.handler, connection, msg, name.as_str())
    }

    fn call_mut<'call>(
        &'call mut self,
        _server: &'call ObjectServer,
        _connection: &'call zbus::Connection,
        _msg: &'call Message,
        _name: MemberName<'call>,
    ) -> DispatchResult2<'call> {
        DispatchResult2::NotFound
    }

    fn introspect_to_writer(&self, writer: &mut dyn FmtWrite, _level: usize) {
        let _ = writer.write_str(PLAYER_INTROSPECT);
    }
}

fn track_matches_current(
    shared: &Arc<Mutex<SharedPlayerState>>,
    track_id: &ObjectPath<'_>,
) -> bool {
    let guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .media
        .metadata
        .as_ref()
        .is_some_and(|meta| safe_track_path(&meta.item_id).as_ref() == *track_id)
}

fn dispatch_player_call<'call>(
    shared: &Arc<Mutex<SharedPlayerState>>,
    handler: &ActionHandler,
    connection: &'call zbus::Connection,
    msg: &'call Message,
    name: &str,
) -> DispatchResult2<'call> {
    let handler = Arc::clone(handler);
    let simple = match name {
        "Play" => Some(OsMediaAction::Play),
        "Pause" => Some(OsMediaAction::Pause),
        "PlayPause" => Some(OsMediaAction::PlayPause),
        "Stop" => Some(OsMediaAction::Stop),
        "Next" => Some(OsMediaAction::Next),
        "Previous" => Some(OsMediaAction::Previous),
        _ => None,
    };
    if let Some(action) = simple {
        return DispatchResult2::new_async(connection, msg, async move {
            handler(action);
            Ok::<(), fdo::Error>(())
        });
    }
    let shared = Arc::clone(shared);
    match name {
        "Seek" => DispatchResult2::new_async(connection, msg, async move {
            let offset_us: i64 = msg
                .body()
                .deserialize()
                .map_err(|err| fdo::Error::InvalidArgs(err.to_string()))?;
            handler(OsMediaAction::SeekDeltaUs(offset_us));
            Ok::<(), fdo::Error>(())
        }),
        "SetPosition" => DispatchResult2::new_async(connection, msg, async move {
            let (track_id, pos_us): (OwnedObjectPath, i64) = msg
                .body()
                .deserialize()
                .map_err(|err| fdo::Error::InvalidArgs(err.to_string()))?;
            if pos_us >= 0 && track_matches_current(&shared, &track_id) {
                handler(OsMediaAction::SetPositionMs((pos_us as f64) / 1000.0));
            }
            Ok::<(), fdo::Error>(())
        }),
        "OpenUri" => {
            DispatchResult2::new_async(connection, msg, async { Ok::<(), fdo::Error>(()) })
        }
        _ => DispatchResult2::NotFound,
    }
}

fn to_owned_value(val: &Value<'_>) -> OwnedValue {
    val.try_to_owned()
        .unwrap_or_else(|_| OwnedValue::from(false))
}

fn root_property(name: &str) -> Option<OwnedValue> {
    match name {
        "Identity" => Some(to_owned_value(&Value::from("MediaFlick Desktop"))),
        "DesktopEntry" => Some(to_owned_value(&Value::from(
            "io.github.phob.MediaFlickDesktop",
        ))),
        "CanQuit" | "CanSetFullscreen" | "HasTrackList" => Some(OwnedValue::from(false)),
        "CanRaise" => Some(OwnedValue::from(true)),
        "SupportedUriSchemes" | "SupportedMimeTypes" => {
            Some(to_owned_value(&Value::from(Vec::<String>::new())))
        }
        _ => None,
    }
}

fn status_text(status: OsPlaybackStatus) -> &'static str {
    match status {
        OsPlaybackStatus::Playing => "Playing",
        OsPlaybackStatus::Paused => "Paused",
        OsPlaybackStatus::Stopped => "Stopped",
    }
}

fn safe_track_path(item_id: &str) -> OwnedObjectPath {
    let suffix: String = item_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(48)
        .collect();
    let suffix = if suffix.is_empty() {
        "none".to_string()
    } else {
        suffix
    };
    let path = format!("/io/github/phob/MediaFlickDesktop/Track/{suffix}");
    OwnedObjectPath::try_from(path).unwrap_or_else(|_| {
        OwnedObjectPath::from(ObjectPath::from_static_str_unchecked(
            "/io/github/phob/Track/0",
        ))
    })
}

fn file_uri(path: &Path) -> Option<String> {
    use std::fmt::Write as _;

    if !path.is_file() {
        return None;
    }
    let abs = path.canonicalize().ok()?;
    let text = abs.to_str()?;
    let mut encoded = String::with_capacity(7 + text.len());
    encoded.push_str("file://");
    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(char::from(*byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    Some(encoded)
}

fn build_metadata(meta: Option<&OsMediaMetadata>) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::new();
    let Some(meta) = meta else {
        return map;
    };
    map.insert(
        "mpris:trackid".to_string(),
        to_owned_value(&Value::from(safe_track_path(&meta.item_id))),
    );
    if meta.duration_us > 0 {
        map.insert(
            "mpris:length".to_string(),
            OwnedValue::from(meta.duration_us),
        );
    }
    map.insert(
        "xesam:title".to_string(),
        to_owned_value(&Value::from(meta.title.as_str())),
    );
    if let Some(artist) = meta.artist.as_deref().filter(|s| !s.is_empty()) {
        map.insert(
            "xesam:artist".to_string(),
            to_owned_value(&Value::from(vec![artist.to_string()])),
        );
    }
    if let Some(album) = meta.album.as_deref().filter(|s| !s.is_empty()) {
        map.insert(
            "xesam:album".to_string(),
            to_owned_value(&Value::from(album)),
        );
    }
    if let Some(url) = meta.artwork_path.as_deref().and_then(file_uri) {
        map.insert(
            "mpris:artUrl".to_string(),
            to_owned_value(&Value::from(url)),
        );
    }
    map
}

fn metadata_value(meta: Option<&OsMediaMetadata>) -> OwnedValue {
    let entries = build_metadata(meta);
    to_owned_value(&Value::from(entries))
}

fn player_property(state: &SharedPlayerState, name: &str) -> Option<OwnedValue> {
    let media = &state.media;
    let has_media = media.metadata.is_some();
    match name {
        "PlaybackStatus" => Some(to_owned_value(&Value::from(status_text(media.status)))),
        "LoopStatus" => Some(to_owned_value(&Value::from("None"))),
        "Rate" | "MinimumRate" | "MaximumRate" => Some(OwnedValue::from(1.0_f64)),
        "Shuffle" => Some(OwnedValue::from(false)),
        "Metadata" => Some(metadata_value(media.metadata.as_ref())),
        "Volume" => Some(OwnedValue::from(media.volume)),
        "Position" => Some(OwnedValue::from(state.extrapolated_position_us())),
        "CanGoNext" => Some(OwnedValue::from(
            media.metadata.as_ref().is_some_and(|m| m.can_go_next),
        )),
        "CanGoPrevious" => Some(OwnedValue::from(
            media.metadata.as_ref().is_some_and(|m| m.can_go_previous),
        )),
        "CanPlay" | "CanPause" => Some(OwnedValue::from(has_media)),
        "CanSeek" => Some(OwnedValue::from(media.can_seek)),
        "CanControl" => Some(OwnedValue::from(true)),
        _ => None,
    }
}

fn player_properties_map(state: &SharedPlayerState) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::new();
    for key in [
        "PlaybackStatus",
        "LoopStatus",
        "Rate",
        "Shuffle",
        "Metadata",
        "Volume",
        "Position",
        "MinimumRate",
        "MaximumRate",
        "CanGoNext",
        "CanGoPrevious",
        "CanPlay",
        "CanPause",
        "CanSeek",
        "CanControl",
    ] {
        if let Some(val) = player_property(state, key) {
            map.insert(key.to_string(), val);
        }
    }
    map
}

struct MprisDiff {
    status: bool,
    metadata: bool,
    volume: bool,
    seeked: bool,
}

fn run_worker(rx: &mpsc::Receiver<WorkerMsg>, handler: ActionHandler) {
    let shared = Arc::new(Mutex::new(SharedPlayerState::default()));
    let conn = match Connection::session() {
        Ok(conn) => conn,
        Err(error) => {
            tracing::debug!(target: "playback.os", "MPRIS2 session bus unavailable: {error}");
            return;
        }
    };
    let root = MprisRoot {
        handler: Arc::clone(&handler),
    };
    let player = MprisPlayer {
        shared: Arc::clone(&shared),
        handler,
    };
    if conn.object_server().at(MPRIS_PATH, root).is_err()
        || conn.object_server().at(MPRIS_PATH, player).is_err()
    {
        tracing::warn!(target: "playback.os", "failed to register MPRIS2 interfaces");
        return;
    }
    if conn.request_name(BUS_PREFIX).is_err() {
        let fallback = format!("{BUS_PREFIX}.instance{}", std::process::id());
        let _ = conn.request_name(fallback.as_str());
    }

    let mut previous_pos_us = 0_i64;
    let mut previous_meta: Option<OsMediaMetadata> = None;
    let mut previous_volume = 1.0_f64;
    let mut previous_status = OsPlaybackStatus::Stopped;
    let mut previous_updated_at = Instant::now();
    while let Ok(msg) = rx.recv() {
        let WorkerMsg::Update(next) = msg else {
            break;
        };
        let now = Instant::now();
        let next_pos = next.position_us;
        let next_status = next.status;
        let meta_changed = next.metadata != previous_meta;
        let status_changed = next_status != previous_status;
        let volume_changed = (next.volume - previous_volume).abs() > 0.001;
        let expected_pos_us = if previous_status == OsPlaybackStatus::Playing {
            let elapsed = i64::try_from(
                now.saturating_duration_since(previous_updated_at)
                    .as_micros(),
            )
            .unwrap_or(0);
            previous_pos_us.saturating_add(elapsed)
        } else {
            previous_pos_us
        };
        let seeked = !meta_changed
            && next_status != OsPlaybackStatus::Stopped
            && (next_pos - expected_pos_us).abs() > 750_000;

        {
            let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
            guard.media = next.clone();
            guard.updated_at = now;
        }
        emit_mpris_updates(
            &conn,
            &next,
            &MprisDiff {
                status: status_changed,
                metadata: meta_changed,
                volume: volume_changed,
                seeked,
            },
        );
        previous_pos_us = next_pos;
        previous_meta = next.metadata;
        previous_volume = next.volume;
        previous_status = next_status;
        previous_updated_at = now;
    }
}

fn emit_mpris_updates(conn: &Connection, next: &OsMediaState, diff: &MprisDiff) {
    let mut changed: HashMap<&str, Value<'_>> = HashMap::new();
    let owned_meta = diff
        .metadata
        .then(|| metadata_value(next.metadata.as_ref()));
    if diff.status {
        changed.insert("PlaybackStatus", Value::from(status_text(next.status)));
        let has_media = next.metadata.is_some();
        changed.insert("CanPlay", Value::from(has_media));
        changed.insert("CanPause", Value::from(has_media));
        changed.insert("CanSeek", Value::from(next.can_seek));
    }
    if diff.volume {
        changed.insert("Volume", Value::from(next.volume));
    }
    if let Some(meta_val) = owned_meta {
        changed.insert("Metadata", Value::from(meta_val));
        changed.insert(
            "CanGoNext",
            Value::from(next.metadata.as_ref().is_some_and(|m| m.can_go_next)),
        );
        changed.insert(
            "CanGoPrevious",
            Value::from(next.metadata.as_ref().is_some_and(|m| m.can_go_previous)),
        );
    }
    if !changed.is_empty() {
        let invalidated: Vec<&str> = Vec::new();
        let _ = conn.emit_signal(
            None::<&str>,
            MPRIS_PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(PLAYER_IFACE, changed, invalidated),
        );
    }
    if diff.seeked {
        let _ = conn.emit_signal(
            None::<&str>,
            MPRIS_PATH,
            PLAYER_IFACE,
            "Seeked",
            &(next.position_us,),
        );
    }
}

const ROOT_INTROSPECT: &str = r#"<interface name="org.mpris.MediaPlayer2">
  <method name="Raise"/>
  <method name="Quit"/>
  <property name="CanQuit" type="b" access="read"/>
  <property name="CanRaise" type="b" access="read"/>
  <property name="CanSetFullscreen" type="b" access="read"/>
  <property name="HasTrackList" type="b" access="read"/>
  <property name="Identity" type="s" access="read"/>
  <property name="DesktopEntry" type="s" access="read"/>
  <property name="SupportedUriSchemes" type="as" access="read"/>
  <property name="SupportedMimeTypes" type="as" access="read"/>
</interface>"#;

const PLAYER_INTROSPECT: &str = r#"<interface name="org.mpris.MediaPlayer2.Player">
  <method name="Next"/>
  <method name="Previous"/>
  <method name="Pause"/>
  <method name="PlayPause"/>
  <method name="Stop"/>
  <method name="Play"/>
  <method name="Seek"><arg name="Offset" type="x" direction="in"/></method>
  <method name="SetPosition"><arg name="TrackId" type="o" direction="in"/><arg name="Position" type="x" direction="in"/></method>
  <signal name="Seeked"><arg name="Position" type="x"/></signal>
  <property name="PlaybackStatus" type="s" access="read"/>
  <property name="LoopStatus" type="s" access="readwrite"/>
  <property name="Rate" type="d" access="readwrite"/>
  <property name="Shuffle" type="b" access="readwrite"/>
  <property name="Metadata" type="a{sv}" access="read"/>
  <property name="Volume" type="d" access="readwrite"/>
  <property name="Position" type="x" access="read"/>
  <property name="MinimumRate" type="d" access="read"/>
  <property name="MaximumRate" type="d" access="read"/>
  <property name="CanGoNext" type="b" access="read"/>
  <property name="CanGoPrevious" type="b" access="read"/>
  <property name="CanPlay" type="b" access="read"/>
  <property name="CanPause" type="b" access="read"/>
  <property name="CanSeek" type="b" access="read"/>
  <property name="CanControl" type="b" access="read"/>
</interface>"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn track_state(item_id: Option<&str>) -> Arc<Mutex<SharedPlayerState>> {
        let state = SharedPlayerState {
            media: OsMediaState {
                status: OsPlaybackStatus::Playing,
                position_us: 0,
                duration_us: 1000,
                volume: 1.0,
                can_seek: true,
                metadata: item_id.map(|id| OsMediaMetadata {
                    item_id: id.to_string(),
                    title: "Title".to_string(),
                    artist: None,
                    album: None,
                    duration_us: 1000,
                    artwork_path: None,
                    is_video: true,
                    can_go_next: false,
                    can_go_previous: false,
                }),
            },
            updated_at: Instant::now(),
        };
        Arc::new(Mutex::new(state))
    }

    #[test]
    fn set_position_track_check_accepts_current_track_and_rejects_stale_track() {
        let shared = track_state(Some("ep-current-2"));
        let current = safe_track_path("ep-current-2");
        let stale = safe_track_path("ep-previous-1");
        assert!(track_matches_current(&shared, &current.as_ref()));
        assert!(!track_matches_current(&shared, &stale.as_ref()));

        let empty = track_state(None);
        assert!(!track_matches_current(&empty, &current.as_ref()));
    }
}
