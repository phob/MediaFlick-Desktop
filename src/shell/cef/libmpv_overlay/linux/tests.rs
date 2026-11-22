use super::*;
use crate::playback::{
    NativeOverlayFrame, NativeOverlayReply, PlaybackRequest, PlayerBackend as PlaybackBackend,
    PlayerSnapshot,
};
use crate::preferences::{FullscreenBehavior, PlayerPreferences};
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

type OverlayRequest = (
    NativeOverlayFrame,
    Sender<Result<NativeOverlayFrame, String>>,
);

struct BlockingOverlayBackend {
    submissions: Mutex<Sender<OverlayRequest>>,
}

impl PlaybackBackend for BlockingOverlayBackend {
    fn warm(&self, _path: String, _fullscreen: FullscreenBehavior) {}
    fn native_window(&self, _timeout: Duration) -> Option<NativeWindowHandle> {
        None
    }
    fn submit_overlay(&self, frame: NativeOverlayFrame) -> Result<NativeOverlayReply, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.submissions
            .lock()
            .expect("submissions lock")
            .send((frame, reply_tx))
            .expect("record overlay submission");
        Ok(reply_rx)
    }
    fn load(&self, _path: String, _fullscreen: FullscreenBehavior, _request: PlaybackRequest) {}
    fn control(&self, _command: PlayerCommand) {}
    fn stop_and_wait(&self, _timeout: Duration) -> bool {
        true
    }
    fn set_preferences(&self, _preferences: PlayerPreferences) {}
    fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot::default()
    }
    fn shutdown(&self) {}
}

fn test_surface() -> (Rc<LibmpvOverlaySurface>, mpsc::Receiver<OverlayRequest>) {
    let (submissions_tx, submissions_rx) = mpsc::channel();
    let (events_tx, _events_rx) = mpsc::channel();
    let playback = Arc::new(PlaybackCoordinator::new(events_tx, move |_| {
        Box::new(BlockingOverlayBackend {
            submissions: Mutex::new(submissions_tx),
        })
    }));
    let settings = AppSettings {
        player_backend: Some(PlayerBackend::Libmpv),
        ..AppSettings::default()
    };
    let state = crate::shell::cef::test_browser_state(Arc::clone(&playback));
    let surface =
        LibmpvOverlaySurface::select(&settings, playback, state).expect("libmpv surface selected");
    (surface, submissions_rx)
}

const TEST_PRESENT_TIMEOUT: Duration = Duration::from_millis(25);

fn set_frame(surface: &LibmpvOverlaySurface, fill: u8) {
    surface.frame_size.set((2, 2));
    *surface.frame.borrow_mut() = vec![fill; 2 * 2 * 4];
}

#[test]
fn dirty_updates_clip_to_the_view_and_preserve_unchanged_pixels() {
    let mut frame = vec![0; 24];
    let pixels = vec![255; 24];
    update_regions(
        &mut frame,
        &pixels,
        3,
        2,
        &[
            Rect {
                x: 1,
                y: -1,
                width: 4,
                height: 2,
            },
            Rect {
                x: i32::MAX,
                y: 0,
                width: i32::MAX,
                height: 1,
            },
        ],
    );
    assert_eq!(&frame[..4], &[0; 4]);
    assert_eq!(&frame[4..12], &[255; 8]);
    assert_eq!(&frame[12..], &[0; 12]);
}

#[test]
fn present_regions_timeout_skips_frame_without_closing_the_surface() {
    let (surface, submissions) = test_surface();
    set_frame(&surface, 10);

    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);

    assert!(!surface.closing.get(), "timeout must not close the surface");
    let (first_frame, first_reply) = submissions
        .recv_timeout(Duration::from_secs(1))
        .expect("first overlay submitted");
    assert_eq!(first_frame.pixels[0], 10);

    set_frame(&surface, 20);
    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);
    assert!(
        !surface.closing.get(),
        "busy skip must not close the surface"
    );
    assert!(
        submissions.try_recv().is_err(),
        "busy coordinator should skip without queuing duplicate frames"
    );

    first_reply.send(Ok(first_frame)).expect("complete first");
    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);
    assert!(!surface.closing.get());
    let (second_frame, second_reply) = submissions
        .recv_timeout(Duration::from_secs(1))
        .expect("next frame submitted after recovery");
    assert_eq!(second_frame.pixels[0], 20);
    second_reply
        .send(Ok(second_frame))
        .expect("complete second");
}

#[test]
fn present_regions_composition_error_closes_the_surface() {
    let (surface, submissions) = test_surface();
    set_frame(&surface, 10);
    let handle = std::thread::spawn(move || {
        let (_frame, reply) = submissions
            .recv_timeout(Duration::from_secs(1))
            .expect("overlay submitted");
        let _ = reply.send(Err("overlay-add failed".to_string()));
    });

    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);
    handle.join().expect("responder finished");

    assert!(surface.closing.get());
}

#[test]
fn present_regions_disconnected_coordinator_closes_the_surface() {
    let (surface, submissions) = test_surface();
    set_frame(&surface, 10);
    let handle = std::thread::spawn(move || {
        let (_frame, reply) = submissions
            .recv_timeout(Duration::from_secs(1))
            .expect("overlay submitted");
        drop(reply);
    });

    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);
    handle.join().expect("responder finished");

    assert!(surface.closing.get());
}

#[test]
fn in_flight_overlay_flushes_deferred_frame_and_drops_stale_resized_buffer() {
    let (surface, submissions) = test_surface();
    set_frame(&surface, 10);

    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);
    assert!(!surface.closing.get());
    let (first_frame, first_reply) = submissions
        .recv_timeout(Duration::from_secs(1))
        .expect("first overlay submitted");

    surface.frame_size.set((3, 2));
    *surface.frame.borrow_mut() = Vec::new();
    first_reply.send(Ok(first_frame)).expect("complete first");
    assert!(surface.flush_in_flight_overlay());
    assert!(
        surface.frame.borrow().is_empty(),
        "stale-sized frame must not overwrite resized buffer"
    );

    set_frame(&surface, 30);
    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);
    let (second_frame, second_reply) = submissions
        .recv_timeout(Duration::from_secs(1))
        .expect("second overlay submitted");
    set_frame(&surface, 40);
    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);
    assert!(surface.deferred_present.get());

    second_reply
        .send(Ok(second_frame))
        .expect("complete second");
    let responder = std::thread::spawn(move || {
        let (third_frame, third_reply) = submissions
            .recv_timeout(Duration::from_secs(1))
            .expect("deferred overlay flushed");
        assert_eq!(third_frame.pixels[0], 40);
        third_reply.send(Ok(third_frame)).expect("complete third");
    });
    assert!(surface.flush_in_flight_overlay());
    responder.join().expect("responder finished");
    assert!(!surface.closing.get());
    assert!(!surface.deferred_present.get());
    assert_eq!(surface.frame.borrow().len(), 2 * 2 * 4);
}

#[test]
fn in_flight_overlay_error_closes_the_surface_on_poll() {
    let (surface, submissions) = test_surface();
    set_frame(&surface, 10);

    surface.present_regions_with_timeout(&[], TEST_PRESENT_TIMEOUT);
    assert!(!surface.closing.get());
    let (_frame, reply) = submissions
        .recv_timeout(Duration::from_secs(1))
        .expect("overlay submitted");
    reply
        .send(Err("overlay-add failed".to_string()))
        .expect("send error");

    assert!(!surface.flush_in_flight_overlay());
    assert!(surface.closing.get());
}
