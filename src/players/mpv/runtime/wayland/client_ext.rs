use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use wayland_client::Proxy as _;
use wayland_client::protocol::{
    wl_compositor, wl_keyboard, wl_pointer, wl_region, wl_seat, wl_shm_pool, wl_subcompositor,
    wl_subsurface, wl_touch,
};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_output_v1, wp_color_management_surface_feedback_v1,
    wp_color_management_surface_v1, wp_color_manager_v1, wp_image_description_creator_icc_v1,
    wp_image_description_creator_params_v1, wp_image_description_info_v1,
    wp_image_description_reference_v1, wp_image_description_v1,
};
use wayland_protocols::wp::color_representation::v1::client::{
    wp_color_representation_manager_v1, wp_color_representation_surface_v1,
};
use wayland_protocols::wp::commit_timing::v1::client::{
    wp_commit_timer_v1, wp_commit_timing_manager_v1,
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1, wp_cursor_shape_manager_v1,
};
use wayland_protocols::wp::fifo::v1::client::{wp_fifo_manager_v1, wp_fifo_v1};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1;
use wayland_protocols::wp::linux_drm_syncobj::v1::client::{
    wp_linux_drm_syncobj_manager_v1, wp_linux_drm_syncobj_surface_v1,
    wp_linux_drm_syncobj_timeline_v1,
};
use wayland_protocols::wp::presentation_time::client::{wp_presentation, wp_presentation_feedback};
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1;
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1;
use wayland_protocols::xdg::shell::client::xdg_toplevel;

use crate::preferences::FullscreenBehavior;

use super::client::{AppState, HeldKey, HostState, HostSurfaceTag, PopupSurface};
use super::{WaylandUiCommand, WaylandUiEvent};

const BTN_LEFT: u32 = 0x110;
const RESIZE_BORDER_PX: i32 = 6;

impl HostState {
    pub fn apply_ui_command(&mut self, command: WaylandUiCommand) -> bool {
        match command {
            WaylandUiCommand::ConfigureWindow {
                title,
                width,
                height,
                maximized,
            } => return self.configure_window(&title, width, height, maximized),
            WaylandUiCommand::Present {
                role,
                logical_width,
                logical_height,
                x,
                y,
                payload,
            } => self.present_ui(role, logical_width, logical_height, x, y, payload),
            WaylandUiCommand::HidePopup => self.hide_popup(),
            WaylandUiCommand::Reveal => {}
            WaylandUiCommand::Minimize => {
                if let Some(s) = self.surfaces.as_ref() {
                    s.xdg_toplevel.set_minimized();
                }
            }
            WaylandUiCommand::SetCursorShape(shape) => self.apply_cursor_shape(shape),
        }
        false
    }

    pub fn configure_window(
        &mut self,
        title: &str,
        width: u16,
        height: u16,
        maximized: bool,
    ) -> bool {
        let Some(s) = self.surfaces.as_ref() else {
            return false;
        };
        s.xdg_toplevel.set_title(title.to_string());
        let mut size_changed = false;
        if !self.user_configured
            && !self.fullscreen
            && !self.maximized
            && width >= 320
            && height >= 240
        {
            self.user_configured = true;
            self.logical_w = width;
            self.logical_h = height;
            s.sync_root_geometry(width, height);
            size_changed = true;
        }
        if maximized != self.maximized {
            if maximized {
                s.xdg_toplevel.set_maximized();
            } else {
                s.xdg_toplevel.unset_maximized();
            }
        }
        self.emit_configure_event();
        size_changed
    }

    pub fn set_fullscreen(&mut self, mode: FullscreenBehavior) {
        let Some(s) = self.surfaces.as_ref() else {
            return;
        };
        if matches!(mode, FullscreenBehavior::Fullscreen) {
            s.xdg_toplevel.set_fullscreen(None);
        } else {
            s.xdg_toplevel.unset_fullscreen();
        }
    }

    fn apply_cursor_shape(&mut self, shape: u32) {
        self.last_cursor_shape = shape.max(1);
        if let Some(device) = self.cursor_device.as_ref()
            && self.pointer_enter_serial != 0
            && let Ok(parsed) = wp_cursor_shape_device_v1::Shape::try_from(self.last_cursor_shape)
        {
            device.set_shape(self.pointer_enter_serial, parsed);
        }
    }

    pub fn hide_popup(&mut self) {
        if let Some(popup) = self.popup.as_ref() {
            popup.surface.attach(None, 0, 0);
            popup.surface.commit();
        }
    }

    pub(super) fn ensure_popup_surface(&mut self) -> Option<&mut PopupSurface> {
        if self.popup.is_none() {
            let s = self.surfaces.as_ref()?;
            let surface = s.compositor.create_surface(&self.qh, HostSurfaceTag::Popup);
            let subsurface = s
                .subcompositor
                .get_subsurface(&surface, &s.ui_surface, &self.qh, ());
            subsurface.set_desync();
            subsurface.place_above(&s.ui_surface);
            let viewport = Some(s.viewporter.get_viewport(&surface, &self.qh, ()));
            self.popup = Some(PopupSurface {
                surface,
                subsurface,
                viewport,
                x: 0,
                y: 0,
            });
        }
        self.popup.as_mut()
    }

    pub fn next_repeat_timeout_ms(&mut self) -> i32 {
        let Some(held) = self.held_key.as_mut() else {
            return -1;
        };
        let now = Instant::now();
        while now >= held.next_fire {
            self.ui.push_event(WaylandUiEvent::Key {
                evdev_code: held.evdev_code,
                base_sym: held.base_sym,
                symbol: held.symbol,
                modifiers: self.modifiers,
                pressed: true,
                repeated: true,
            });
            held.next_fire += held.interval;
        }
        let rem = held.next_fire.saturating_duration_since(now).as_millis();
        i32::try_from(rem.max(1)).unwrap_or(100)
    }

    fn resize_edge_at(&self, x: i32, y: i32) -> xdg_toplevel::ResizeEdge {
        let w = i32::from(self.logical_w);
        let h = i32::from(self.logical_h);
        let left = x <= RESIZE_BORDER_PX;
        let right = x >= w - RESIZE_BORDER_PX;
        let top = y <= RESIZE_BORDER_PX;
        let bottom = y >= h - RESIZE_BORDER_PX;
        match (left, right, top, bottom) {
            (true, _, true, _) => xdg_toplevel::ResizeEdge::TopLeft,
            (_, true, true, _) => xdg_toplevel::ResizeEdge::TopRight,
            (true, _, _, true) => xdg_toplevel::ResizeEdge::BottomLeft,
            (_, true, _, true) => xdg_toplevel::ResizeEdge::BottomRight,
            (true, _, _, _) => xdg_toplevel::ResizeEdge::Left,
            (_, true, _, _) => xdg_toplevel::ResizeEdge::Right,
            (_, _, true, _) => xdg_toplevel::ResizeEdge::Top,
            (_, _, _, true) => xdg_toplevel::ResizeEdge::Bottom,
            _ => xdg_toplevel::ResizeEdge::None,
        }
    }

    pub fn flush_wheel(&mut self) {
        let dx = if self.wheel_x_120 != 0 {
            self.wheel_x_120
        } else {
            let v = (self.wheel_x_acc * 8.0).round() as i32;
            self.wheel_x_acc = 0.0;
            v
        };
        let dy = if self.wheel_y_120 != 0 {
            self.wheel_y_120
        } else {
            let v = (self.wheel_y_acc * 8.0).round() as i32;
            self.wheel_y_acc = 0.0;
            v
        };
        self.wheel_x_120 = 0;
        self.wheel_y_120 = 0;
        if dx != 0 || dy != 0 {
            self.ui.push_event(WaylandUiEvent::PointerWheel {
                x: self.pointer_x,
                y: self.pointer_y,
                delta_x: -dx,
                delta_y: -dy,
                modifiers: self.modifiers,
            });
        }
    }

    pub fn emit_configure_event(&mut self) {
        self.ui.push_event(WaylandUiEvent::Configure {
            width: self.logical_w,
            height: self.logical_h,
            scale_120: self.scale_120,
            fullscreen: self.fullscreen,
            maximized: self.maximized,
            visible: true,
        });
    }
}

delegate_noop!(AppState: ignore wl_compositor::WlCompositor);
delegate_noop!(AppState: ignore wl_subcompositor::WlSubcompositor);
delegate_noop!(AppState: ignore wl_subsurface::WlSubsurface);
delegate_noop!(AppState: ignore wl_region::WlRegion);
delegate_noop!(AppState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(AppState: ignore wp_viewporter::WpViewporter);
delegate_noop!(AppState: ignore wp_viewport::WpViewport);
delegate_noop!(AppState: ignore wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(AppState: ignore wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1);
delegate_noop!(AppState: ignore zxdg_decoration_manager_v1::ZxdgDecorationManagerV1);
delegate_noop!(AppState: ignore wp_cursor_shape_manager_v1::WpCursorShapeManagerV1);
delegate_noop!(AppState: ignore wp_cursor_shape_device_v1::WpCursorShapeDeviceV1);
delegate_noop!(AppState: ignore wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1);
delegate_noop!(AppState: ignore wp_linux_drm_syncobj_surface_v1::WpLinuxDrmSyncobjSurfaceV1);
delegate_noop!(AppState: ignore wp_linux_drm_syncobj_timeline_v1::WpLinuxDrmSyncobjTimelineV1);
delegate_noop!(AppState: ignore wp_fifo_manager_v1::WpFifoManagerV1);
delegate_noop!(AppState: ignore wp_fifo_v1::WpFifoV1);
delegate_noop!(AppState: ignore wp_commit_timing_manager_v1::WpCommitTimingManagerV1);
delegate_noop!(AppState: ignore wp_commit_timer_v1::WpCommitTimerV1);
delegate_noop!(AppState: ignore wp_color_management_surface_v1::WpColorManagementSurfaceV1);
delegate_noop!(AppState: ignore wp_color_representation_surface_v1::WpColorRepresentationSurfaceV1);
delegate_noop!(AppState: ignore wp_image_description_creator_icc_v1::WpImageDescriptionCreatorIccV1);
delegate_noop!(AppState: ignore wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1);
delegate_noop!(AppState: ignore wp_image_description_reference_v1::WpImageDescriptionReferenceV1);
delegate_noop!(AppState: ignore wl_touch::WlTouch);

impl Dispatch<wl_seat::WlSeat, ()> for AppState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_seat::Event::Capabilities { capabilities } => {
                let bits = u32::from(capabilities);
                state.host.seat_caps = bits;
                let has_ptr = (bits & u32::from(wl_seat::Capability::Pointer)) != 0;
                if has_ptr && state.host.pointer.is_none() {
                    let ptr = seat.get_pointer(qh, ());
                    state.host.cursor_device = state
                        .host
                        .globals
                        .cursor_shape_mgr
                        .as_ref()
                        .map(|mgr| mgr.get_pointer(&ptr, qh, ()));
                    state.host.pointer = Some(ptr);
                } else if !has_ptr {
                    if let Some(dev) = state.host.cursor_device.take() {
                        dev.destroy();
                    }
                    if let Some(ptr) = state.host.pointer.take() {
                        ptr.release();
                    }
                }
                let has_kbd = (bits & u32::from(wl_seat::Capability::Keyboard)) != 0;
                if has_kbd && state.host.keyboard.is_none() {
                    state.host.keyboard = Some(seat.get_keyboard(qh, ()));
                } else if !has_kbd && let Some(kbd) = state.host.keyboard.take() {
                    kbd.release();
                    state.host.held_key = None;
                }
            }
            wl_seat::Event::Name { name } => state.host.seat_name = name,
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => {
                state.host.pointer_enter_serial = serial;
                state.host.pointer_on_popup = state
                    .host
                    .popup
                    .as_ref()
                    .is_some_and(|p| p.surface == surface);
                update_pointer_coords(&mut state.host, surface_x, surface_y);
                let shape = state.host.last_cursor_shape;
                if let Some(dev) = state.host.cursor_device.as_ref()
                    && let Ok(parsed) = wp_cursor_shape_device_v1::Shape::try_from(shape)
                {
                    dev.set_shape(serial, parsed);
                }
                state.host.ui.push_event(WaylandUiEvent::PointerMove {
                    x: state.host.pointer_x,
                    y: state.host.pointer_y,
                    modifiers: state.host.modifiers,
                    leave: false,
                });
            }
            wl_pointer::Event::Leave { .. } => {
                state.host.ui.push_event(WaylandUiEvent::PointerMove {
                    x: state.host.pointer_x,
                    y: state.host.pointer_y,
                    modifiers: state.host.modifiers,
                    leave: true,
                });
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                update_pointer_coords(&mut state.host, surface_x, surface_y);
                state.host.ui.push_event(WaylandUiEvent::PointerMove {
                    x: state.host.pointer_x,
                    y: state.host.pointer_y,
                    modifiers: state.host.modifiers,
                    leave: false,
                });
            }
            wl_pointer::Event::Button {
                serial,
                button,
                state: btn_state,
                ..
            } => {
                let pressed = matches!(btn_state, WEnum::Value(wl_pointer::ButtonState::Pressed));
                update_button_modifier(&mut state.host, button, pressed);
                if pressed
                    && button == BTN_LEFT
                    && !state.host.has_server_decoration
                    && !state.host.fullscreen
                    && !state.host.maximized
                    && !state.host.pointer_on_popup
                    && let Some(s) = state.host.surfaces.as_ref()
                    && let Some(seat) = s.seat.as_ref()
                {
                    let edge = state
                        .host
                        .resize_edge_at(state.host.pointer_x, state.host.pointer_y);
                    if !matches!(edge, xdg_toplevel::ResizeEdge::None) {
                        s.xdg_toplevel.resize(seat, serial, edge);
                        return;
                    }
                }
                state.host.ui.push_event(WaylandUiEvent::PointerButton {
                    x: state.host.pointer_x,
                    y: state.host.pointer_y,
                    button,
                    pressed,
                    modifiers: state.host.modifiers,
                });
            }
            wl_pointer::Event::Axis { axis, value, .. } => match axis {
                WEnum::Value(wl_pointer::Axis::VerticalScroll) => state.host.wheel_y_acc += value,
                WEnum::Value(wl_pointer::Axis::HorizontalScroll) => state.host.wheel_x_acc += value,
                _ => {}
            },
            wl_pointer::Event::AxisValue120 { axis, value120 } => match axis {
                WEnum::Value(wl_pointer::Axis::VerticalScroll) => {
                    state.host.wheel_y_120 += value120
                }
                WEnum::Value(wl_pointer::Axis::HorizontalScroll) => {
                    state.host.wheel_x_120 += value120
                }
                _ => {}
            },
            wl_pointer::Event::Frame => state.host.flush_wheel(),
            _ => {}
        }
    }
}

fn update_pointer_coords(host: &mut HostState, sx: f64, sy: f64) {
    let (ox, oy) = if host.pointer_on_popup {
        host.popup.as_ref().map_or((0, 0), |p| (p.x, p.y))
    } else {
        (0, 0)
    };
    host.pointer_x = ox.saturating_add(sx.round() as i32);
    host.pointer_y = oy.saturating_add(sy.round() as i32);
}

fn update_button_modifier(host: &mut HostState, button: u32, pressed: bool) {
    match button {
        BTN_LEFT => host.modifiers.left_button = pressed,
        0x111 => host.modifiers.right_button = pressed,
        0x112 => host.modifiers.middle_button = pressed,
        _ => {}
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if matches!(format, WEnum::Value(wl_keyboard::KeymapFormat::XkbV1))
                    && let Some(km) = state.host.keymap.as_mut()
                {
                    km.load_fd(fd, size);
                }
            }
            wl_keyboard::Event::Enter { .. } => {
                state
                    .host
                    .ui
                    .push_event(WaylandUiEvent::KeyboardFocus(true));
            }
            wl_keyboard::Event::Leave { .. } => {
                state.host.held_key = None;
                state
                    .host
                    .ui
                    .push_event(WaylandUiEvent::KeyboardFocus(false));
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(km) = state.host.keymap.as_mut() {
                    km.update_mask(mods_depressed, mods_latched, mods_locked, group);
                    state.host.modifiers = km.modifiers(state.host.modifiers);
                }
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                state.host.repeat_rate = rate;
                state.host.repeat_delay = delay;
            }
            wl_keyboard::Event::Key {
                key, state: kstate, ..
            } => {
                let pressed = matches!(kstate, WEnum::Value(wl_keyboard::KeyState::Pressed));
                let repeated_event = u32::from(kstate) == 2;
                let (base_sym, symbol) = state.host.keymap.as_mut().map_or((0, 0), |km| {
                    km.translate_key(key, pressed || repeated_event)
                });
                if pressed && !repeated_event && state.host.repeat_rate > 0 {
                    let delay_ms = u64::try_from(state.host.repeat_delay.max(50)).unwrap_or(400);
                    let interval_ms =
                        (1000 / u64::try_from(state.host.repeat_rate).unwrap_or(25)).max(16);
                    state.host.held_key = Some(HeldKey {
                        evdev_code: key,
                        base_sym,
                        symbol,
                        next_fire: Instant::now() + Duration::from_millis(delay_ms),
                        interval: Duration::from_millis(interval_ms),
                    });
                } else if !pressed
                    && state
                        .host
                        .held_key
                        .as_ref()
                        .is_some_and(|h| h.evdev_code == key)
                {
                    state.host.held_key = None;
                }
                state.host.ui.push_event(WaylandUiEvent::Key {
                    evdev_code: key,
                    base_sym,
                    symbol,
                    modifiers: state.host.modifiers,
                    pressed: pressed || repeated_event,
                    repeated: repeated_event,
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_presentation::WpPresentation, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wp_presentation::WpPresentation,
        event: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation::Event::ClockId { clk_id } = event {
            state.host.presentation_clock_id = Some(clk_id);
        }
    }
}

impl Dispatch<wp_presentation_feedback::WpPresentationFeedback, ()> for AppState {
    fn event(
        state: &mut Self,
        fb: &wp_presentation_feedback::WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_presentation_feedback::Event::SyncOutput { output } => {
                if let Some(c_fb) = state.bridge.host_pres_feedbacks.get(&fb.id())
                    && let Some(gn) = state
                        .host
                        .outputs
                        .iter()
                        .find(|o| o.output.as_ref().is_some_and(|ho| ho.id() == output.id()))
                        .map(|o| o.global_name)
                {
                    for (cgn, weak_co) in &state.bridge.client_outputs {
                        if *cgn == gn
                            && let Ok(co) = weak_co.upgrade()
                        {
                            c_fb.sync_output(&co);
                        }
                    }
                }
            }
            wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                refresh,
                seq_hi,
                seq_lo,
                flags,
            } => {
                if let Some(c_fb) = state.bridge.host_pres_feedbacks.remove(&fb.id()) {
                    let sflags = wayland_protocols::wp::presentation_time::server::wp_presentation_feedback::Kind::from_bits_truncate(u32::from(flags));
                    c_fb.presented(
                        tv_sec_hi, tv_sec_lo, tv_nsec, refresh, seq_hi, seq_lo, sflags,
                    );
                }
            }
            wp_presentation_feedback::Event::Discarded => {
                if let Some(c_fb) = state.bridge.host_pres_feedbacks.remove(&fb.id()) {
                    c_fb.discarded();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_color_manager_v1::WpColorManagerV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wp_color_manager_v1::WpColorManagerV1,
        event: wp_color_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_color_manager_v1::Event::SupportedIntent { render_intent } => {
                state.host.cm_intents.push(u32::from(render_intent));
            }
            wp_color_manager_v1::Event::SupportedFeature { feature } => {
                state.host.cm_features.push(u32::from(feature));
            }
            wp_color_manager_v1::Event::SupportedTfNamed { tf } => {
                state.host.cm_tfs.push(u32::from(tf));
            }
            wp_color_manager_v1::Event::SupportedPrimariesNamed { primaries } => {
                state.host.cm_primaries.push(u32::from(primaries));
            }
            wp_color_manager_v1::Event::Done => state.host.cm_done = true,
            _ => {}
        }
    }
}

impl Dispatch<wp_color_management_output_v1::WpColorManagementOutputV1, ()> for AppState {
    fn event(
        state: &mut Self,
        obj: &wp_color_management_output_v1::WpColorManagementOutputV1,
        event: wp_color_management_output_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_color_management_output_v1::Event::ImageDescriptionChanged = event
            && let Some(c_out) = state.bridge.host_cm_outputs.get(&obj.id())
        {
            c_out.image_description_changed();
        }
    }
}

impl Dispatch<wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1, ()>
    for AppState
{
    fn event(
        state: &mut Self,
        obj: &wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1,
        event: wp_color_management_surface_feedback_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(c_fb) = state.bridge.host_cm_feedbacks.get(&obj.id()) else {
            return;
        };
        match event {
            wp_color_management_surface_feedback_v1::Event::PreferredChanged { identity } => {
                c_fb.preferred_changed(identity);
            }
            wp_color_management_surface_feedback_v1::Event::PreferredChanged2 {
                identity_hi,
                identity_lo,
            } => {
                c_fb.preferred_changed2(identity_hi, identity_lo);
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_image_description_v1::WpImageDescriptionV1, ()> for AppState {
    fn event(
        state: &mut Self,
        obj: &wp_image_description_v1::WpImageDescriptionV1,
        event: wp_image_description_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, c_desc)) = state.bridge.host_img_descs.get(&obj.id()) else {
            return;
        };
        match event {
            wp_image_description_v1::Event::Failed { cause, msg } => {
                let c = wayland_protocols::wp::color_management::v1::server::wp_image_description_v1::Cause::try_from(u32::from(cause))
                    .unwrap_or(wayland_protocols::wp::color_management::v1::server::wp_image_description_v1::Cause::Unsupported);
                c_desc.failed(c, msg);
            }
            wp_image_description_v1::Event::Ready { identity } => {
                c_desc.ready(identity);
            }
            wp_image_description_v1::Event::Ready2 {
                identity_hi,
                identity_lo,
            } => {
                c_desc.ready2(identity_hi, identity_lo);
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_image_description_info_v1::WpImageDescriptionInfoV1, ()> for AppState {
    fn event(
        state: &mut Self,
        obj: &wp_image_description_info_v1::WpImageDescriptionInfoV1,
        event: wp_image_description_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_protocols::wp::color_management::v1::server::wp_color_manager_v1::{
            Primaries as s_prim, TransferFunction as s_tf,
        };
        let Some(c_info) = state.bridge.host_img_infos.get(&obj.id()) else {
            return;
        };
        match event {
            wp_image_description_info_v1::Event::Done => {
                c_info.done();
                state.bridge.host_img_infos.remove(&obj.id());
            }
            wp_image_description_info_v1::Event::IccFile { icc, icc_size } => {
                c_info.icc_file(icc.as_fd(), icc_size);
            }
            wp_image_description_info_v1::Event::Primaries {
                r_x,
                r_y,
                g_x,
                g_y,
                b_x,
                b_y,
                w_x,
                w_y,
            } => c_info.primaries(r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y),
            wp_image_description_info_v1::Event::PrimariesNamed { primaries } => {
                if let Ok(p) = s_prim::try_from(u32::from(primaries)) {
                    c_info.primaries_named(p);
                }
            }
            wp_image_description_info_v1::Event::TfPower { eexp } => c_info.tf_power(eexp),
            wp_image_description_info_v1::Event::TfNamed { tf } => {
                if let Ok(t) = s_tf::try_from(u32::from(tf)) {
                    c_info.tf_named(t);
                }
            }
            wp_image_description_info_v1::Event::Luminances {
                min_lum,
                max_lum,
                reference_lum,
            } => c_info.luminances(min_lum, max_lum, reference_lum),
            wp_image_description_info_v1::Event::TargetPrimaries {
                r_x,
                r_y,
                g_x,
                g_y,
                b_x,
                b_y,
                w_x,
                w_y,
            } => c_info.target_primaries(r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y),
            wp_image_description_info_v1::Event::TargetLuminance { min_lum, max_lum } => {
                c_info.target_luminance(min_lum, max_lum);
            }
            wp_image_description_info_v1::Event::TargetMaxCll { max_cll } => {
                c_info.target_max_cll(max_cll);
            }
            wp_image_description_info_v1::Event::TargetMaxFall { max_fall } => {
                c_info.target_max_fall(max_fall);
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_color_representation_manager_v1::WpColorRepresentationManagerV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wp_color_representation_manager_v1::WpColorRepresentationManagerV1,
        event: wp_color_representation_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_color_representation_manager_v1::Event::SupportedAlphaMode { alpha_mode } => {
                state.host.cr_alpha_modes.push(u32::from(alpha_mode));
            }
            wp_color_representation_manager_v1::Event::SupportedCoefficientsAndRanges {
                coefficients,
                range,
            } => {
                state
                    .host
                    .cr_coeffs_ranges
                    .push((u32::from(coefficients), u32::from(range)));
            }
            wp_color_representation_manager_v1::Event::Done => state.host.cr_done = true,
            _ => {}
        }
    }
}
