//! An input-only sibling above the embedded video container. Browser pixels are composed by
//! libmpv itself, so this window never allocates a second video-sized surface.
use cef::{ImplBinaryValue, ImplImage};
use std::cell::Cell;
use std::io;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub(super) struct OverlayWindow {
    pub conn: RustConnection,
    pub parent: u32,
    pub window: u32,
    pub root: u32,
    pub delete: u32,
    cursor_font: u32,
    content: u32,
    resize: Cell<ResizeState>,
}

impl OverlayWindow {
    pub fn new(parent: u32, content: u32) -> io::Result<Self> {
        let (conn, screen_index) = x11rb::connect(None).map_err(io::Error::other)?;
        let screen = &conn.setup().roots[screen_index];
        let root = screen.root;
        let window = conn.generate_id().map_err(io::Error::other)?;
        let mask = EventMask::STRUCTURE_NOTIFY
            | EventMask::EXPOSURE
            | EventMask::POINTER_MOTION
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::KEY_PRESS
            | EventMask::KEY_RELEASE
            | EventMask::ENTER_WINDOW
            | EventMask::LEAVE_WINDOW
            | EventMask::FOCUS_CHANGE;
        conn.create_window(
            0,
            window,
            parent,
            0,
            0,
            1280,
            720,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(mask),
        )
        .map_err(io::Error::other)?
        .check()
        .map_err(io::Error::other)?;
        let delete = atom(&conn, b"WM_DELETE_WINDOW")?;
        let cursor_font = conn.generate_id().map_err(io::Error::other)?;
        conn.open_font(cursor_font, b"cursor")
            .map_err(io::Error::other)?;
        let result = Self {
            conn,
            parent,
            window,
            root,
            delete,
            cursor_font,
            content,
            resize: Cell::new(ResizeState::new((1280, 720))),
        };
        result.set_identity()?;
        super::input::enable_detectable_repeat(&result.conn)?;
        result
            .conn
            .change_window_attributes(
                parent,
                &ChangeWindowAttributesAux::new().event_mask(
                    EventMask::STRUCTURE_NOTIFY
                        | EventMask::PROPERTY_CHANGE
                        | EventMask::FOCUS_CHANGE,
                ),
            )
            .map_err(io::Error::other)?;
        result.conn.flush().map_err(io::Error::other)?;
        Ok(result)
    }

    fn set_identity(&self) -> io::Result<()> {
        self.conn
            .change_window_attributes(
                self.root,
                &ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
            )
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub fn is_aligned(&self, expected: &super::Geometry) -> io::Result<bool> {
        let geometry = self
            .conn
            .get_geometry(self.window)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        let position = self
            .conn
            .translate_coordinates(self.window, self.root, 0, 0)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        Ok(position.dst_x == expected.x
            && position.dst_y == expected.y
            && geometry.width == expected.width
            && geometry.height == expected.height)
    }

    pub fn resize_content(&self, width: u16, height: u16) -> io::Result<()> {
        self.conn
            .configure_window(
                self.content,
                &ConfigureWindowAux::new()
                    .width(u32::from(width))
                    .height(u32::from(height)),
            )
            .map_err(io::Error::other)?
            .check()
            .map_err(io::Error::other)?;
        self.resize.set(ResizeState::new((width, height)));
        self.conn.flush().map_err(io::Error::other)
    }

    pub fn resize_settled(&self) -> bool {
        self.resize.get().pending.is_none()
    }

    pub fn geometry(&self) -> io::Result<(i16, i16, u16, u16, bool)> {
        let geometry = self
            .conn
            .get_geometry(self.parent)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        let mut resize = self.resize.get();
        if let Some((width, height)) =
            resize.observe((geometry.width, geometry.height), Instant::now())
        {
            self.resize_content(width, height)?;
        }
        self.resize.set(resize);
        let (width, height) = resize.committed;
        let position = self
            .conn
            .translate_coordinates(self.content, self.root, 0, 0)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        let attributes = self
            .conn
            .get_window_attributes(self.parent)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        Ok((
            position.dst_x,
            position.dst_y,
            width,
            height,
            attributes.map_state == MapState::VIEWABLE,
        ))
    }

    pub fn place(&self, width: u16, height: u16, visible: bool) -> io::Result<()> {
        self.conn
            .configure_window(
                self.window,
                &ConfigureWindowAux::new()
                    .x(0)
                    .y(0)
                    .width(u32::from(width))
                    .height(u32::from(height))
                    .stack_mode(StackMode::ABOVE),
            )
            .map_err(io::Error::other)?;
        if visible {
            self.conn
                .map_window(self.window)
                .map_err(io::Error::other)?;
        } else {
            self.conn
                .unmap_window(self.window)
                .map_err(io::Error::other)?;
        }
        self.conn.flush().map_err(io::Error::other)
    }

    pub fn activate(&self) -> io::Result<()> {
        let event = ClientMessageEvent::new(
            32,
            self.parent,
            atom(&self.conn, b"_NET_ACTIVE_WINDOW")?,
            [1, x11rb::CURRENT_TIME, 0, 0, 0],
        );
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(io::Error::other)?;
        self.conn.flush().map_err(io::Error::other)
    }

    /// Treat the native frame, video child, and browser input child as one
    /// focus scope. X11 emits FocusOut during transfers between these windows
    /// and during grabs even though the application still owns keyboard focus.
    pub fn focus_browser(&self) -> io::Result<bool> {
        let focus = self
            .conn
            .get_input_focus()
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?
            .focus;
        if focus == self.window {
            return Ok(true);
        }
        if !self.owns_focus(focus)? {
            return Ok(false);
        }
        self.conn
            .set_input_focus(InputFocus::PARENT, self.window, x11rb::CURRENT_TIME)
            .map_err(io::Error::other)?
            .check()
            .map_err(io::Error::other)?;
        Ok(true)
    }

    fn owns_focus(&self, mut focus: u32) -> io::Result<bool> {
        // gpu-next creates its own child inside content. Treat that subtree as
        // the same focus scope, including after the VO recreates its surface.
        for _ in 0..32 {
            if focus == self.parent || focus == self.content {
                return Ok(true);
            }
            if focus == self.root || focus <= 1 {
                return Ok(false);
            }
            focus = self
                .conn
                .query_tree(focus)
                .map_err(io::Error::other)?
                .reply()
                .map_err(io::Error::other)?
                .parent;
        }
        Ok(false)
    }

    pub fn restore(&self, settings: crate::preferences::WebUiWindowSettings) -> io::Result<()> {
        use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
        let hints = WmSizeHints {
            position: settings
                .position()
                .map(|(x, y)| (WmSizeHintsSpecification::UserSpecified, x, y)),
            size: Some((
                WmSizeHintsSpecification::UserSpecified,
                settings.width,
                settings.height,
            )),
            win_gravity: Some(Gravity::STATIC),
            ..WmSizeHints::default()
        };
        hints
            .set_normal_hints(&self.conn, self.parent)
            .map_err(io::Error::other)?;
        if settings.maximized {
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    self.parent,
                    atom(&self.conn, b"_NET_WM_STATE")?,
                    AtomEnum::ATOM,
                    &[
                        atom(&self.conn, b"_NET_WM_STATE_MAXIMIZED_VERT")?,
                        atom(&self.conn, b"_NET_WM_STATE_MAXIMIZED_HORZ")?,
                    ],
                )
                .map_err(io::Error::other)?;
        }
        Ok(())
    }

    pub fn set_title(&self, title: &str) -> io::Result<()> {
        let name = atom(&self.conn, b"_NET_WM_NAME")?;
        let utf8 = atom(&self.conn, b"UTF8_STRING")?;
        for window in [self.parent, self.window] {
            self.conn
                .change_property8(PropMode::REPLACE, window, name, utf8, title.as_bytes())
                .map_err(io::Error::other)?;
        }
        self.conn.flush().map_err(io::Error::other)
    }

    pub fn set_icon(&self) {
        let Some(image) = cef::image_create() else {
            return;
        };
        if image.add_png(
            1.0,
            Some(include_bytes!(
                "../../../../../distribution/linux/app-icon-256.png"
            )),
        ) != 1
        {
            return;
        }
        let (mut width, mut height) = (0, 0);
        let Some(bitmap) = image.as_bitmap(
            1.0,
            cef::ColorType::BGRA_8888,
            cef::AlphaType::POSTMULTIPLIED,
            Some(&mut width),
            Some(&mut height),
        ) else {
            return;
        };
        let mut bytes = vec![0; bitmap.size()];
        if bitmap.data(Some(&mut bytes), 0) != bytes.len() {
            return;
        }
        let mut icon = vec![width as u32, height as u32];
        icon.extend(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
        );
        if let Ok(property) = atom(&self.conn, b"_NET_WM_ICON") {
            let _ = self.conn.change_property32(
                PropMode::REPLACE,
                self.parent,
                property,
                AtomEnum::CARDINAL,
                &icon,
            );
        }
    }

    pub fn state(&self) -> io::Result<(bool, bool)> {
        let property = atom(&self.conn, b"_NET_WM_STATE")?;
        let reply = self
            .conn
            .get_property(false, self.parent, property, AtomEnum::ATOM, 0, 32)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        let states: Vec<_> = reply.value32().into_iter().flatten().collect();
        Ok((
            states.contains(&atom(&self.conn, b"_NET_WM_STATE_FULLSCREEN")?),
            states.contains(&atom(&self.conn, b"_NET_WM_STATE_MAXIMIZED_VERT")?)
                && states.contains(&atom(&self.conn, b"_NET_WM_STATE_MAXIMIZED_HORZ")?),
        ))
    }

    pub fn cursor(&self, glyph: Option<u16>) -> io::Result<()> {
        let cursor = self.conn.generate_id().map_err(io::Error::other)?;
        if let Some(glyph) = glyph {
            self.conn
                .create_glyph_cursor(
                    cursor,
                    self.cursor_font,
                    self.cursor_font,
                    glyph,
                    glyph + 1,
                    0,
                    0,
                    0,
                    65535,
                    65535,
                    65535,
                )
                .map_err(io::Error::other)?;
        } else {
            let pixmap = self.conn.generate_id().map_err(io::Error::other)?;
            let gc = self.conn.generate_id().map_err(io::Error::other)?;
            self.conn
                .create_pixmap(1, pixmap, self.root, 1, 1)
                .map_err(io::Error::other)?;
            self.conn
                .create_gc(gc, pixmap, &CreateGCAux::new().foreground(0))
                .map_err(io::Error::other)?;
            self.conn
                .poly_fill_rectangle(
                    pixmap,
                    gc,
                    &[Rectangle {
                        x: 0,
                        y: 0,
                        width: 1,
                        height: 1,
                    }],
                )
                .map_err(io::Error::other)?;
            self.conn
                .create_cursor(cursor, pixmap, pixmap, 0, 0, 0, 0, 0, 0, 0, 0)
                .map_err(io::Error::other)?;
            self.conn.free_gc(gc).map_err(io::Error::other)?;
            self.conn.free_pixmap(pixmap).map_err(io::Error::other)?;
        }
        self.conn
            .change_window_attributes(
                self.window,
                &ChangeWindowAttributesAux::new().cursor(cursor),
            )
            .map_err(io::Error::other)?;
        self.conn.free_cursor(cursor).map_err(io::Error::other)?;
        self.conn.flush().map_err(io::Error::other)
    }
}

pub(super) fn atom(conn: &RustConnection, name: &[u8]) -> io::Result<u32> {
    Ok(conn
        .intern_atom(false, name)
        .map_err(io::Error::other)?
        .reply()
        .map_err(io::Error::other)?
        .atom)
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        let _ = self.conn.destroy_window(self.window);
        let _ = self.conn.close_font(self.cursor_font);
        let _ = self.conn.flush();
    }
}

// Keep intermediate WM sizes away from the video renderer. In particular,
// entering fullscreen can first produce a work-area size, then the full size.
#[derive(Clone, Copy)]
struct ResizeState {
    committed: (u16, u16),
    pending: Option<((u16, u16), Instant)>,
}

impl ResizeState {
    const QUIET: Duration = Duration::from_millis(250);

    fn new(committed: (u16, u16)) -> Self {
        Self {
            committed,
            pending: None,
        }
    }

    fn observe(&mut self, size: (u16, u16), now: Instant) -> Option<(u16, u16)> {
        if size == self.committed {
            self.pending = None;
            return None;
        }
        match self.pending {
            Some((target, since)) if target == size => {
                if now.duration_since(since) >= Self::QUIET {
                    self.committed = size;
                    self.pending = None;
                    return Some(size);
                }
            }
            _ => self.pending = Some((size, now)),
        }
        None
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated X11 display (run under Xvfb)"]
    fn internal_focus_transfers_and_grabs_keep_browser_focus()
    -> Result<(), Box<dyn std::error::Error>> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let parent = conn.generate_id()?;
        let content = conn.generate_id()?;
        let outside = conn.generate_id()?;
        let video = conn.generate_id()?;
        for (window, owner) in [
            (parent, root),
            (content, parent),
            (video, content),
            (outside, root),
        ] {
            conn.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                owner,
                0,
                0,
                1280,
                720,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().override_redirect(1),
            )?
            .check()?;
            conn.map_window(window)?.check()?;
        }
        let overlay = OverlayWindow::new(parent, content)?;
        overlay.place(1280, 720, true)?;
        overlay.conn.get_input_focus()?.reply()?;

        // A window-manager click focuses the frame; the adapter redirects to
        // its input child. Both transfers emit FocusOut, but neither is a blur.
        for target in [parent, content, video, overlay.window, parent] {
            conn.set_input_focus(InputFocus::PARENT, target, x11rb::CURRENT_TIME)?
                .check()?;
            assert!(overlay.focus_browser()?);
            assert_eq!(conn.get_input_focus()?.reply()?.focus, overlay.window);
        }

        // A temporary keyboard grab changes event routing, not logical focus.
        let grab = conn
            .grab_keyboard(
                false,
                outside,
                x11rb::CURRENT_TIME,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?
            .reply()?;
        assert_eq!(grab.status, GrabStatus::SUCCESS);
        assert!(overlay.focus_browser()?);
        conn.ungrab_keyboard(x11rb::CURRENT_TIME)?.check()?;
        assert!(overlay.focus_browser()?);

        // Genuine focus loss still reaches CEF so menus and key state reset.
        conn.set_input_focus(InputFocus::PARENT, outside, x11rb::CURRENT_TIME)?
            .check()?;
        assert!(!overlay.focus_browser()?);
        assert_eq!(conn.get_input_focus()?.reply()?.focus, outside);
        drop(overlay);
        conn.destroy_window(parent)?.check()?;
        conn.destroy_window(outside)?.check()?;
        Ok(())
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;

    #[test]
    fn fullscreen_commits_only_the_final_size_after_the_quiet_period() {
        let quiet = ResizeState::QUIET;
        let work_area = Instant::now();
        let mut state = ResizeState::new((4182, 2846));
        assert_eq!(state.observe((6144, 3382), work_area), None);
        let full = work_area + quiet / 2;
        assert_eq!(state.observe((6144, 3456), full), None);
        assert_eq!(state.observe((6144, 3456), full + quiet / 2), None);
        assert_eq!(
            state.observe((6144, 3456), full + quiet),
            Some((6144, 3456))
        );
        assert_eq!(state.observe((6144, 3456), full + quiet * 2), None);
    }

    #[test]
    fn returning_to_current_size_cancels_a_pending_resize() {
        let now = Instant::now();
        let mut state = ResizeState::new((1280, 720));
        state.observe((3840, 2160), now);
        state.observe((1280, 720), now + Duration::from_millis(100));
        assert_eq!(
            state.observe((3840, 2160), now + Duration::from_millis(500)),
            None
        );
        assert_eq!(state.committed, (1280, 720));
    }
}
