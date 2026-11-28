use wayland_client::QueueHandle;
use wayland_client::protocol::{
    wl_compositor, wl_output, wl_registry, wl_seat, wl_shm, wl_subcompositor,
};
use wayland_protocols::wp::color_management::v1::client::wp_color_manager_v1;
use wayland_protocols::wp::color_representation::v1::client::wp_color_representation_manager_v1;
use wayland_protocols::wp::commit_timing::v1::client::wp_commit_timing_manager_v1;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1;
use wayland_protocols::wp::fifo::v1::client::wp_fifo_manager_v1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1;
use wayland_protocols::wp::linux_drm_syncobj::v1::client::wp_linux_drm_syncobj_manager_v1;
use wayland_protocols::wp::presentation_time::client::wp_presentation;
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1;
use wayland_protocols::wp::viewporter::client::wp_viewporter;
use wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1;
use wayland_protocols::xdg::shell::client::xdg_wm_base;

use super::client::{AppState, DmabufFeedbackTag, HostState, OutputInfo};

#[derive(Clone, Default)]
pub(super) struct ExtensionManagers {
    pub fractional_scale_mgr: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    pub presentation: Option<wp_presentation::WpPresentation>,
    pub syncobj_mgr: Option<wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1>,
    pub fifo_mgr: Option<wp_fifo_manager_v1::WpFifoManagerV1>,
    pub commit_timing_mgr: Option<wp_commit_timing_manager_v1::WpCommitTimingManagerV1>,
    pub color_mgr: Option<wp_color_manager_v1::WpColorManagerV1>,
    pub color_repr_mgr: Option<wp_color_representation_manager_v1::WpColorRepresentationManagerV1>,
}

#[derive(Default)]
pub(super) struct HostGlobals {
    pub compositor: Option<wl_compositor::WlCompositor>,
    pub subcompositor: Option<wl_subcompositor::WlSubcompositor>,
    pub shm: Option<wl_shm::WlShm>,
    pub seat: Option<wl_seat::WlSeat>,
    pub xdg_wm_base: Option<xdg_wm_base::XdgWmBase>,
    pub viewporter: Option<wp_viewporter::WpViewporter>,
    pub dmabuf: Option<(zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, u32)>,
    pub single_pixel_mgr: Option<wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1>,
    pub decoration_mgr: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1>,
    pub cursor_shape_mgr: Option<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1>,
    pub ext: ExtensionManagers,
}

pub(super) fn bind_global(
    host: &mut HostState,
    registry: &wl_registry::WlRegistry,
    name: u32,
    interface: &str,
    version: u32,
    qh: &QueueHandle<AppState>,
) {
    if !bind_core_global(host, registry, name, interface, version, qh) {
        bind_extension_global(&mut host.globals, registry, name, interface, version, qh);
    }
}

fn bind_core_global(
    host: &mut HostState,
    registry: &wl_registry::WlRegistry,
    name: u32,
    interface: &str,
    version: u32,
    qh: &QueueHandle<AppState>,
) -> bool {
    let g = &mut host.globals;
    match interface {
        "wl_compositor" => {
            g.compositor = Some(registry.bind::<wl_compositor::WlCompositor, _, _>(
                name,
                version.min(6),
                qh,
                (),
            ));
        }
        "wl_subcompositor" => {
            g.subcompositor =
                Some(registry.bind::<wl_subcompositor::WlSubcompositor, _, _>(name, 1, qh, ()));
        }
        "wl_shm" => {
            g.shm = Some(registry.bind::<wl_shm::WlShm, _, _>(name, version.min(2), qh, ()));
        }
        "wl_seat" => {
            g.seat = Some(registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(7), qh, ()));
        }
        "xdg_wm_base" => {
            g.xdg_wm_base =
                Some(registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, version.min(6), qh, ()));
        }
        "wp_viewporter" => {
            g.viewporter =
                Some(registry.bind::<wp_viewporter::WpViewporter, _, _>(name, 1, qh, ()));
        }
        "zwp_linux_dmabuf_v1" => {
            let v = version.min(5);
            let dmabuf =
                registry.bind::<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, _, _>(name, v, qh, ());
            if v >= 4 {
                host.default_feedback =
                    Some(dmabuf.get_default_feedback(qh, DmabufFeedbackTag::Default));
            }
            g.dmabuf = Some((dmabuf, v));
        }
        "wl_output" => {
            let out = registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, name);
            host.outputs.push(OutputInfo {
                global_name: name,
                output: Some(out),
                scale: 1,
                ..OutputInfo::default()
            });
        }
        _ => return false,
    }
    true
}

fn bind_extension_global(
    g: &mut HostGlobals,
    registry: &wl_registry::WlRegistry,
    name: u32,
    interface: &str,
    version: u32,
    qh: &QueueHandle<AppState>,
) {
    match interface {
        "wp_fractional_scale_manager_v1" => {
            g.ext.fractional_scale_mgr = Some(
                registry.bind::<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, _, _>(
                    name,
                    1,
                    qh,
                    (),
                ),
            );
        }
        "wp_single_pixel_buffer_manager_v1" => {
            g.single_pixel_mgr = Some(
                registry
                    .bind::<wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1, _, _>(
                        name,
                        1,
                        qh,
                        (),
                    ),
            );
        }
        "zxdg_decoration_manager_v1" => {
            g.decoration_mgr = Some(
                registry.bind::<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1, _, _>(
                    name,
                    1,
                    qh,
                    (),
                ),
            );
        }
        "wp_cursor_shape_manager_v1" => {
            g.cursor_shape_mgr = Some(
                registry.bind::<wp_cursor_shape_manager_v1::WpCursorShapeManagerV1, _, _>(
                    name,
                    version.min(2),
                    qh,
                    (),
                ),
            );
        }
        "wp_presentation" => {
            g.ext.presentation = Some(registry.bind::<wp_presentation::WpPresentation, _, _>(
                name,
                version.min(2),
                qh,
                (),
            ));
        }
        "wp_linux_drm_syncobj_manager_v1" => {
            g.ext.syncobj_mgr = Some(
                registry.bind::<wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1, _, _>(
                    name,
                    1,
                    qh,
                    (),
                ),
            );
        }
        "wp_fifo_manager_v1" => {
            g.ext.fifo_mgr =
                Some(registry.bind::<wp_fifo_manager_v1::WpFifoManagerV1, _, _>(name, 1, qh, ()));
        }
        "wp_commit_timing_manager_v1" => {
            g.ext.commit_timing_mgr = Some(
                registry.bind::<wp_commit_timing_manager_v1::WpCommitTimingManagerV1, _, _>(
                    name,
                    1,
                    qh,
                    (),
                ),
            );
        }
        "wp_color_manager_v1" => {
            g.ext.color_mgr = Some(
                registry.bind::<wp_color_manager_v1::WpColorManagerV1, _, _>(
                    name,
                    version.min(3),
                    qh,
                    (),
                ),
            );
        }
        "wp_color_representation_manager_v1" => {
            g.ext.color_repr_mgr = Some(
                registry
                    .bind::<wp_color_representation_manager_v1::WpColorRepresentationManagerV1, _, _>(
                        name, 1, qh, (),
                    ),
            );
        }
        _ => {}
    }
}

pub(super) fn apply_output_event(info: &mut OutputInfo, event: wl_output::Event) {
    match event {
        wl_output::Event::Geometry {
            x,
            y,
            physical_width,
            physical_height,
            subpixel,
            make,
            model,
            transform,
        } => {
            info.x = x;
            info.y = y;
            info.physical_width = physical_width;
            info.physical_height = physical_height;
            info.subpixel = i32::try_from(u32::from(subpixel)).unwrap_or(0);
            info.make = make;
            info.model = model;
            info.transform = i32::try_from(u32::from(transform)).unwrap_or(0);
        }
        wl_output::Event::Mode {
            flags,
            width,
            height,
            refresh,
        } => {
            info.mode_flags = u32::from(flags);
            info.mode_width = width;
            info.mode_height = height;
            info.mode_refresh = refresh;
        }
        wl_output::Event::Scale { factor } => info.scale = factor,
        wl_output::Event::Name { name } => info.name = name,
        wl_output::Event::Description { description } => info.description = description,
        wl_output::Event::Done => info.done = true,
        _ => {}
    }
}
