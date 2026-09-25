//! Read the physical/logical output relationship from the Wayland compositor.
//! XWayland's integer DPI can oversample fractional-scale monitors; the UI
//! needs that DPI for layout, but doesn't need more pixels than the output.
use std::collections::BTreeMap;

use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, delegate_noop, protocol::*};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1, zxdg_output_v1,
};

#[derive(Default)]
pub(super) struct Output {
    position: (i32, i32),
    logical: (i32, i32),
    physical: (i32, i32),
}

impl Output {
    fn density(&self) -> Option<f64> {
        let (width, height) = self.logical;
        let (pw, ph) = self.physical;
        if [width, height, pw, ph].into_iter().any(|v| v <= 0) {
            return None;
        }
        // Output modes are unrotated; logical dimensions include rotation.
        for (pw, ph) in [(pw, ph), (ph, pw)] {
            let x = f64::from(pw) / f64::from(width);
            let y = f64::from(ph) / f64::from(height);
            if (0.5..=8.0).contains(&x) && (x - y).abs() < 0.01 {
                return Some(x);
            }
        }
        None
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.position.0
            && y >= self.position.1
            && i64::from(x) < i64::from(self.position.0) + i64::from(self.logical.0)
            && i64::from(y) < i64::from(self.position.1) + i64::from(self.logical.1)
    }
}

pub(super) fn density_at(outputs: &[Output], x: i32, y: i32) -> Option<f64> {
    outputs
        .iter()
        .find(|output| output.contains(x, y))
        .and_then(Output::density)
}

pub(super) fn outputs() -> Vec<Output> {
    read_outputs().unwrap_or_default()
}

fn read_outputs() -> Option<Vec<Output>> {
    let connection = Connection::connect_to_env().ok()?;
    let mut queue = connection.new_event_queue();
    let handle = queue.handle();
    let _registry = connection.display().get_registry(&handle, ());
    let mut state = State::default();
    // Registry globals, bound outputs, then xdg logical geometry. No surfaces,
    // input devices, or desktop settings are created or modified.
    for _ in 0..3 {
        queue.roundtrip(&mut state).ok()?;
    }
    Some(
        state
            .outputs
            .into_values()
            .map(|(_, output)| output)
            .collect(),
    )
}

#[derive(Default)]
struct State {
    manager: Option<ZxdgOutputManagerV1>,
    outputs: BTreeMap<u32, (wl_output::WlOutput, Output)>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _connection: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_output" => {
                    let output = registry.bind::<wl_output::WlOutput, _, _>(
                        name,
                        version.min(4),
                        handle,
                        name,
                    );
                    if let Some(manager) = &state.manager {
                        manager.get_xdg_output(&output, handle, name);
                    }
                    state.outputs.insert(name, (output, Output::default()));
                }
                "zxdg_output_manager_v1" => {
                    let manager = registry.bind::<ZxdgOutputManagerV1, _, _>(
                        name,
                        version.min(3),
                        handle,
                        (),
                    );
                    for (&name, (output, _)) in &state.outputs {
                        manager.get_xdg_output(output, handle, name);
                    }
                    state.manager = Some(manager);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        name: &u32,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode {
            flags: WEnum::Value(flags),
            width,
            height,
            ..
        } = event
            && flags.contains(wl_output::Mode::Current)
            && let Some((_, output)) = state.outputs.get_mut(name)
        {
            output.physical = (width, height);
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for State {
    fn event(
        state: &mut Self,
        _proxy: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        name: &u32,
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        if let Some((_, output)) = state.outputs.get_mut(name) {
            match event {
                zxdg_output_v1::Event::LogicalSize { width, height } => {
                    output.logical = (width, height)
                }
                zxdg_output_v1::Event::LogicalPosition { x, y } => output.position = (x, y),
                _ => {}
            }
        }
    }
}

delegate_noop!(State: ignore ZxdgOutputManagerV1);
