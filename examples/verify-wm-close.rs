#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::*;

    const ICONIC_STATE: u32 = 3;
    let mut args = std::env::args().skip(1);
    let class = args
        .next()
        .ok_or("usage: verify-wm-close <WM_CLASS class> [watch-ms]")?;
    let watch = Duration::from_millis(args.next().map_or(Ok(0), |ms| ms.parse())?);
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
    let delete = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
    let change_state = conn.intern_atom(false, b"WM_CHANGE_STATE")?.reply()?.atom;
    let mut targets = Vec::new();
    for window in conn.query_tree(root)?.reply()?.children {
        if conn.get_window_attributes(window)?.reply()?.map_state != MapState::VIEWABLE {
            continue;
        }
        let wm_class = conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)?
            .reply()?
            .value;
        if wm_class.split(|byte| *byte == 0).nth(1) != Some(class.as_bytes()) {
            continue;
        }
        let advertised = conn
            .get_property(false, window, protocols, AtomEnum::ATOM, 0, 32)?
            .reply()?;
        if advertised
            .value32()
            .into_iter()
            .flatten()
            .any(|atom| atom == delete)
        {
            targets.push(window);
        }
    }
    let [window] = targets[..] else {
        return Err(format!(
            "expected one viewable top-level of class {class} with WM_DELETE_WINDOW, found {targets:?}"
        )
        .into());
    };
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
    )?
    .check()?;
    let message = ClientMessageEvent::new(
        32,
        window,
        protocols,
        [delete, x11rb::CURRENT_TIME, 0, 0, 0],
    );
    conn.send_event(false, window, EventMask::NO_EVENT, message)?
        .check()?;
    conn.flush()?;
    let deadline = Instant::now() + watch;
    let mut iconify_requested = false;
    while !iconify_requested && Instant::now() < deadline {
        match conn.poll_for_event()? {
            Some(Event::ClientMessage(event)) => {
                iconify_requested = event.window == window
                    && event.type_ == change_state
                    && event.data.as_data32()[0] == ICONIC_STATE;
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    println!("{{\"window\":\"0x{window:x}\",\"iconifyRequested\":{iconify_requested}}}");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("verify-wm-close is X11-only");
    std::process::exit(2);
}
