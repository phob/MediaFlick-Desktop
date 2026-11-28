use super::{NowPlaying, PlayerSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InhibitLevel {
    None,
    IdleOnly,
    DisplayAndIdle,
}

pub trait InhibitBackend: Send {
    fn apply(&mut self, level: InhibitLevel);
}

pub fn inhibit_level_for(snapshot: &PlayerSnapshot) -> InhibitLevel {
    if !snapshot.active || snapshot.paused {
        return InhibitLevel::None;
    }
    match snapshot.now_playing {
        Some(NowPlaying::Video) => InhibitLevel::DisplayAndIdle,
        Some(NowPlaying::Audio(_)) => InhibitLevel::IdleOnly,
        None => InhibitLevel::None,
    }
}

pub struct SleepInhibitor {
    current: InhibitLevel,
    backend: Box<dyn InhibitBackend>,
}

impl SleepInhibitor {
    pub fn new() -> Self {
        Self::with_backend(platform_backend())
    }

    pub fn with_backend(backend: Box<dyn InhibitBackend>) -> Self {
        Self {
            current: InhibitLevel::None,
            backend,
        }
    }

    pub fn update(&mut self, snapshot: &PlayerSnapshot) {
        let target = inhibit_level_for(snapshot);
        if target != self.current {
            self.backend.apply(target);
            self.current = target;
        }
    }

    pub fn clear(&mut self) {
        if self.current != InhibitLevel::None {
            self.backend.apply(InhibitLevel::None);
            self.current = InhibitLevel::None;
        }
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(target_os = "linux")]
fn platform_backend() -> Box<dyn InhibitBackend> {
    Box::new(LinuxInhibitor::default())
}

#[cfg(target_os = "windows")]
fn platform_backend() -> Box<dyn InhibitBackend> {
    Box::new(WindowsInhibitor::default())
}

#[cfg(target_os = "macos")]
fn platform_backend() -> Box<dyn InhibitBackend> {
    Box::new(MacosInhibitor::default())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn platform_backend() -> Box<dyn InhibitBackend> {
    struct NoopInhibitor;
    impl InhibitBackend for NoopInhibitor {
        fn apply(&mut self, _level: InhibitLevel) {}
    }
    Box::new(NoopInhibitor)
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct LinuxInhibitor {
    system: Option<zbus::blocking::Connection>,
    session: Option<zbus::blocking::Connection>,
    logind_fd: Option<zbus::zvariant::OwnedFd>,
    screensaver_cookie: Option<u32>,
}

#[cfg(target_os = "linux")]
impl LinuxInhibitor {
    fn release(&mut self) {
        self.logind_fd = None;
        if let (Some(cookie), Some(session)) =
            (self.screensaver_cookie.take(), self.session.as_ref())
        {
            let _ = session.call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "UnInhibit",
                &(cookie,),
            );
        }
    }

    fn system_conn(&mut self) -> Option<&zbus::blocking::Connection> {
        if self.system.is_none() {
            self.system = zbus::blocking::Connection::system().ok();
        }
        self.system.as_ref()
    }

    fn session_conn(&mut self) -> Option<&zbus::blocking::Connection> {
        if self.session.is_none() {
            self.session = zbus::blocking::Connection::session().ok();
        }
        self.session.as_ref()
    }
}

#[cfg(target_os = "linux")]
impl InhibitBackend for LinuxInhibitor {
    fn apply(&mut self, level: InhibitLevel) {
        self.release();
        let (what, reason, inhibit_screen) = match level {
            InhibitLevel::None => return,
            InhibitLevel::IdleOnly => ("sleep", "Audio playback in progress", false),
            InhibitLevel::DisplayAndIdle => ("idle:sleep", "Video playback in progress", true),
        };
        if let Some(conn) = self.system_conn() {
            match conn.call_method(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                Some("org.freedesktop.login1.Manager"),
                "Inhibit",
                &(what, "MediaFlick Desktop", reason, "block"),
            ) {
                Ok(reply) => {
                    self.logind_fd = reply.body().deserialize::<zbus::zvariant::OwnedFd>().ok();
                }
                Err(error) => {
                    tracing::debug!(target: "playback.os", "login1 Inhibit failed: {error}");
                }
            }
        }
        if inhibit_screen && let Some(conn) = self.session_conn() {
            match conn.call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "Inhibit",
                &("MediaFlick Desktop", reason),
            ) {
                Ok(reply) => {
                    self.screensaver_cookie = reply.body().deserialize::<u32>().ok();
                }
                Err(error) => {
                    tracing::debug!(target: "playback.os", "ScreenSaver Inhibit failed: {error}");
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct WindowsInhibitor {
    handle: usize,
    active_types: Vec<i32>,
}

#[cfg(target_os = "windows")]
impl WindowsInhibitor {
    fn ensure_request(&mut self) -> Option<windows_sys::Win32::Foundation::HANDLE> {
        use windows_sys::Win32::System::Power::PowerCreateRequest;
        use windows_sys::Win32::System::Threading::{
            POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
        };

        if self.handle != 0 {
            return Some(self.handle as windows_sys::Win32::Foundation::HANDLE);
        }
        let mut reason: Vec<u16> = "Media playback in progress\0".encode_utf16().collect();
        let context = REASON_CONTEXT {
            Version: 0,
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 {
                SimpleReasonString: reason.as_mut_ptr(),
            },
        };
        // SAFETY: `context` points to a valid null-terminated UTF-16 string for the duration of `PowerCreateRequest`.
        let handle = unsafe { PowerCreateRequest(&context) };
        if handle.is_null() {
            return None;
        }
        self.handle = handle as usize;
        Some(handle)
    }

    fn clear_active(&mut self) {
        use windows_sys::Win32::System::Power::PowerClearRequest;

        if self.handle == 0 {
            self.active_types.clear();
            return;
        }
        let handle = self.handle as windows_sys::Win32::Foundation::HANDLE;
        for prev in self.active_types.drain(..) {
            // SAFETY: `self.handle` is a valid handle returned by `PowerCreateRequest`.
            unsafe {
                let _ = PowerClearRequest(handle, prev);
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl InhibitBackend for WindowsInhibitor {
    fn apply(&mut self, level: InhibitLevel) {
        use windows_sys::Win32::System::Power::{
            PowerRequestDisplayRequired, PowerRequestSystemRequired, PowerSetRequest,
        };

        self.clear_active();
        let req_types: &[i32] = match level {
            InhibitLevel::None => return,
            InhibitLevel::IdleOnly => &[PowerRequestSystemRequired],
            InhibitLevel::DisplayAndIdle => {
                &[PowerRequestDisplayRequired, PowerRequestSystemRequired]
            }
        };
        if let Some(handle) = self.ensure_request() {
            for &req_type in req_types {
                // SAFETY: `handle` is valid and `req_type` is a documented `POWER_REQUEST_TYPE`.
                let ok = unsafe { PowerSetRequest(handle, req_type) };
                if ok != 0 {
                    self.active_types.push(req_type);
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsInhibitor {
    fn drop(&mut self) {
        self.apply(InhibitLevel::None);
        if self.handle != 0 {
            // SAFETY: `self.handle` was created by `PowerCreateRequest` and has not been closed yet.
            unsafe {
                let _ = windows_sys::Win32::Foundation::CloseHandle(
                    self.handle as windows_sys::Win32::Foundation::HANDLE,
                );
            }
            self.handle = 0;
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct MacosInhibitor {
    assertion_id: Option<u32>,
}

#[cfg(target_os = "macos")]
impl MacosInhibitor {
    fn release(&mut self) {
        #[link(name = "IOKit", kind = "framework")]
        unsafe extern "C" {
            fn IOPMAssertionRelease(assertion_id: u32) -> i32;
        }
        if let Some(id) = self.assertion_id.take() {
            // SAFETY: `id` was obtained from `IOPMAssertionCreateWithName`.
            unsafe {
                let _ = IOPMAssertionRelease(id);
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl InhibitBackend for MacosInhibitor {
    fn apply(&mut self, level: InhibitLevel) {
        use std::ffi::{c_char, c_void};

        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFStringCreateWithCString(
                alloc: *const c_void,
                c_str: *const c_char,
                encoding: u32,
            ) -> *const c_void;
            fn CFRelease(cf: *const c_void);
        }
        #[link(name = "IOKit", kind = "framework")]
        unsafe extern "C" {
            fn IOPMAssertionCreateWithName(
                assertion_type: *const c_void,
                assertion_level: u32,
                assertion_name: *const c_void,
                assertion_id: *mut u32,
            ) -> i32;
        }

        const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
        const IO_PM_ASSERTION_LEVEL_ON: u32 = 255;

        self.release();
        let kind = match level {
            InhibitLevel::None => return,
            InhibitLevel::IdleOnly => c"PreventUserIdleSystemSleep",
            InhibitLevel::DisplayAndIdle => c"PreventUserIdleDisplaySleep",
        };
        // SAFETY: passing static null-terminated UTF-8 strings to CoreFoundation and IOKit, then releasing the temporary `CFStringRef`s.
        unsafe {
            let cf_type =
                CFStringCreateWithCString(std::ptr::null(), kind.as_ptr(), CF_STRING_ENCODING_UTF8);
            let cf_name = CFStringCreateWithCString(
                std::ptr::null(),
                c"MediaFlick Desktop Playback".as_ptr(),
                CF_STRING_ENCODING_UTF8,
            );
            if !cf_type.is_null() && !cf_name.is_null() {
                let mut id = 0_u32;
                let status = IOPMAssertionCreateWithName(
                    cf_type,
                    IO_PM_ASSERTION_LEVEL_ON,
                    cf_name,
                    &mut id,
                );
                if status == 0 && id != 0 {
                    self.assertion_id = Some(id);
                }
            }
            if !cf_type.is_null() {
                CFRelease(cf_type);
            }
            if !cf_name.is_null() {
                CFRelease(cf_name);
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacosInhibitor {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::playback::model::{
        AudioDisplay, AudioKind, AudioNowPlaying, AudioPhase, PlaylistItemId,
    };

    struct RecordingBackend {
        calls: Arc<Mutex<Vec<InhibitLevel>>>,
    }

    impl InhibitBackend for RecordingBackend {
        fn apply(&mut self, level: InhibitLevel) {
            self.calls.lock().expect("lock").push(level);
        }
    }

    fn audio_now_playing() -> NowPlaying {
        let track = AudioDisplay {
            item_id: "track-1".into(),
            title: "Track".into(),
            album_id: None,
            album: None,
            artists: vec![],
            track_number: None,
            disc_number: None,
            runtime_ticks: None,
            artwork: None,
        };
        NowPlaying::Audio(Box::new(AudioNowPlaying {
            media_kind: AudioKind::Music,
            current: track,
            next: None,
            playlist_item_id: PlaylistItemId::new(1, 0),
            index: 0,
            length: 1,
            can_previous: false,
            can_next: false,
            phase: AudioPhase::Playing,
        }))
    }

    #[test]
    fn inhibit_level_tracks_media_kind_and_pause_state() {
        let mut video = PlayerSnapshot {
            active: true,
            paused: false,
            now_playing: Some(NowPlaying::Video),
            ..PlayerSnapshot::default()
        };
        assert_eq!(inhibit_level_for(&video), InhibitLevel::DisplayAndIdle);
        video.paused = true;
        assert_eq!(inhibit_level_for(&video), InhibitLevel::None);

        let mut audio = PlayerSnapshot {
            active: true,
            paused: false,
            now_playing: Some(audio_now_playing()),
            ..PlayerSnapshot::default()
        };
        assert_eq!(inhibit_level_for(&audio), InhibitLevel::IdleOnly);
        audio.paused = true;
        assert_eq!(inhibit_level_for(&audio), InhibitLevel::None);

        audio.paused = false;
        audio.active = false;
        assert_eq!(inhibit_level_for(&audio), InhibitLevel::None);
    }

    #[test]
    fn sleep_inhibitor_deduplicates_updates_and_releases_on_drop() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        {
            let mut inhibitor = SleepInhibitor::with_backend(Box::new(RecordingBackend {
                calls: Arc::clone(&calls),
            }));
            let playing_video = PlayerSnapshot {
                active: true,
                paused: false,
                now_playing: Some(NowPlaying::Video),
                ..PlayerSnapshot::default()
            };
            inhibitor.update(&playing_video);
            inhibitor.update(&playing_video);
            let paused = PlayerSnapshot {
                paused: true,
                ..playing_video.clone()
            };
            inhibitor.update(&paused);
            inhibitor.update(&playing_video);
        }
        assert_eq!(
            *calls.lock().expect("lock"),
            vec![
                InhibitLevel::DisplayAndIdle,
                InhibitLevel::None,
                InhibitLevel::DisplayAndIdle,
                InhibitLevel::None,
            ]
        );
    }
}
