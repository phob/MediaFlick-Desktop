//! App-owned X11 toplevel and a separately sized video container. The shell
//! coalesces WM resize events before resizing that container and libmpv's VO.
use std::io;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::playback::NativeWindowHandle;
use crate::preferences::FullscreenBehavior;

pub(super) struct HostWindow {
    conn: RustConnection,
    root: u32,
    window: u32,
    content: u32,
    delete: u32,
    protocols: u32,
}

impl HostWindow {
    pub fn new() -> io::Result<Self> {
        let (conn, screen) = x11rb::connect(None).map_err(io::Error::other)?;
        let root = conn.setup().roots[screen].root;
        let window = conn.generate_id().map_err(io::Error::other)?;
        let content = conn.generate_id().map_err(io::Error::other)?;
        for (id, parent) in [(window, root), (content, window)] {
            conn.create_window(
                0,
                id,
                parent,
                0,
                0,
                1280,
                720,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().background_pixel(0).border_pixel(0),
            )
            .map_err(io::Error::other)?
            .check()
            .map_err(io::Error::other)?;
        }
        let delete = conn
            .intern_atom(false, b"WM_DELETE_WINDOW")
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?
            .atom;
        let protocols = conn
            .intern_atom(false, b"WM_PROTOCOLS")
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?
            .atom;
        let host = Self {
            conn,
            root,
            window,
            content,
            delete,
            protocols,
        };
        host.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                host.protocols,
                AtomEnum::ATOM,
                &[host.delete],
            )
            .map_err(io::Error::other)?;
        host.conn
            .change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                b"io.github.phob.MediaFlickDesktop\0io.github.phob.MediaFlickDesktop\0",
            )
            .map_err(io::Error::other)?;
        host.conn.map_window(content).map_err(io::Error::other)?;
        host.conn.flush().map_err(io::Error::other)?;
        Ok(host)
    }

    pub fn handle(&self) -> io::Result<NativeWindowHandle> {
        NativeWindowHandle::with_content(self.window as usize, self.content as usize)
            .ok_or_else(|| io::Error::other("invalid native video container"))
    }

    pub fn content_size(&self) -> io::Result<(u16, u16)> {
        let geometry = self
            .conn
            .get_geometry(self.content)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        Ok((geometry.width, geometry.height))
    }

    fn atom(&self, name: &[u8]) -> io::Result<u32> {
        Ok(self
            .conn
            .intern_atom(false, name)
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?
            .atom)
    }

    pub fn poll_events(&self) {
        for _ in 0..64 {
            match self.conn.poll_for_event() {
                Ok(Some(x11rb::protocol::Event::ClientMessage(event)))
                    if event.window == self.window
                        && event.format == 32
                        && event.type_ == self.protocols
                        && event.data.as_data32()[0] == self.delete =>
                {
                    super::request_window_close();
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(target: "mpv.window", "native window connection failed: {error}");
                    super::request_window_close();
                    break;
                }
            }
        }
    }

    pub fn fullscreen(&self) -> io::Result<bool> {
        let fullscreen = self.atom(b"_NET_WM_STATE_FULLSCREEN")?;
        let state = self
            .conn
            .get_property(
                false,
                self.window,
                self.atom(b"_NET_WM_STATE")?,
                AtomEnum::ATOM,
                0,
                32,
            )
            .map_err(io::Error::other)?
            .reply()
            .map_err(io::Error::other)?;
        Ok(state
            .value32()
            .into_iter()
            .flatten()
            .any(|atom| atom == fullscreen))
    }

    pub fn set_fullscreen(&self, mode: FullscreenBehavior) -> io::Result<()> {
        let message = ClientMessageEvent::new(
            32,
            self.window,
            self.atom(b"_NET_WM_STATE")?,
            [
                u32::from(mode == FullscreenBehavior::Fullscreen),
                self.atom(b"_NET_WM_STATE_FULLSCREEN")?,
                0,
                1,
                0,
            ],
        );
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                message,
            )
            .map_err(io::Error::other)?;
        self.conn.flush().map_err(io::Error::other)
    }
}

impl Drop for HostWindow {
    fn drop(&mut self) {
        let _ = self.conn.destroy_window(self.window);
        let _ = self.conn.flush();
    }
}
