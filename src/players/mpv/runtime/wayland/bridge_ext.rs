use std::os::fd::AsFd;

use wayland_client::Proxy as _;
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_manager_v1 as h_cm, wp_image_description_v1 as h_desc,
};
use wayland_protocols::wp::color_management::v1::server::{
    wp_color_management_output_v1, wp_color_management_surface_feedback_v1,
    wp_color_management_surface_v1, wp_color_manager_v1, wp_image_description_creator_icc_v1,
    wp_image_description_creator_params_v1, wp_image_description_info_v1,
    wp_image_description_reference_v1, wp_image_description_v1,
};
use wayland_protocols::wp::color_representation::v1::client::wp_color_representation_surface_v1 as h_crs;
use wayland_protocols::wp::color_representation::v1::server::{
    wp_color_representation_manager_v1, wp_color_representation_surface_v1,
};
use wayland_protocols::wp::commit_timing::v1::server::{
    wp_commit_timer_v1, wp_commit_timing_manager_v1,
};
use wayland_protocols::wp::fifo::v1::server::{wp_fifo_manager_v1, wp_fifo_v1};
use wayland_protocols::wp::linux_drm_syncobj::v1::server::{
    wp_linux_drm_syncobj_manager_v1, wp_linux_drm_syncobj_surface_v1,
    wp_linux_drm_syncobj_timeline_v1,
};
use wayland_protocols::wp::presentation_time::server::{wp_presentation, wp_presentation_feedback};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

use super::client::{AppState, HostSurfaces};

pub(super) fn register_extension_globals(dh: &DisplayHandle, s: &HostSurfaces) {
    if let Some(p) = s.ext.presentation.as_ref() {
        dh.create_global::<AppState, wp_presentation::WpPresentation, _>(
            p.version().clamp(1, 2),
            (),
        );
    }
    if s.ext.syncobj_mgr.is_some() {
        dh.create_global::<AppState, wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1, _>(
            1,
            (),
        );
    }
    if s.ext.fifo_mgr.is_some() {
        dh.create_global::<AppState, wp_fifo_manager_v1::WpFifoManagerV1, _>(1, ());
    }
    if s.ext.commit_timing_mgr.is_some() {
        dh.create_global::<AppState, wp_commit_timing_manager_v1::WpCommitTimingManagerV1, _>(
            1,
            (),
        );
    }
    if s.ext.color_repr_mgr.is_some() {
        dh.create_global::<AppState, wp_color_representation_manager_v1::WpColorRepresentationManagerV1, _>(1, ());
    }
    if let Some(cm) = s.ext.color_mgr.as_ref() {
        dh.create_global::<AppState, wp_color_manager_v1::WpColorManagerV1, _>(
            cm.version().clamp(1, 3),
            (),
        );
    }
}

impl GlobalDispatch<wp_presentation::WpPresentation, ()> for AppState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wp_presentation::WpPresentation>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let pres = data_init.init(resource, ());
        if let Some(clk_id) = state.host.presentation_clock_id {
            pres.clock_id(clk_id);
        }
    }
}

impl Dispatch<wp_presentation::WpPresentation, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_presentation::WpPresentation,
        request: wp_presentation::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_presentation::Request::Feedback { callback, .. } = request {
            let client_fb = data_init.init(callback, ());
            if let Some(s) = state.host.surfaces.as_ref()
                && let Some(host_pres) = s.ext.presentation.as_ref()
            {
                let host_fb = host_pres.feedback(&s.video_surface, &state.host.qh, ());
                state
                    .bridge
                    .host_pres_feedbacks
                    .insert(host_fb.id(), client_fb);
            } else {
                client_fb.discarded();
            }
        }
    }
}

impl Dispatch<wp_presentation_feedback::WpPresentationFeedback, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wp_presentation_feedback::WpPresentationFeedback,
        _: wp_presentation_feedback::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1, ()> for AppState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1,
        request: wp_linux_drm_syncobj_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let Some(s) = state.host.surfaces.as_ref() else {
            return;
        };
        let Some(mgr) = s.ext.syncobj_mgr.as_ref() else {
            return;
        };
        match request {
            wp_linux_drm_syncobj_manager_v1::Request::GetSurface { id, .. } => {
                data_init.init(id, ());
                if state.bridge.video_syncobj_surface.is_none() {
                    let sync_surf = mgr.get_surface(&s.video_surface, &state.host.qh, ());
                    state.bridge.video_syncobj_surface = Some(sync_surf);
                }
            }
            wp_linux_drm_syncobj_manager_v1::Request::ImportTimeline { id, fd } => {
                let tl = data_init.init(id, ());
                let h_tl = mgr.import_timeline(fd.as_fd(), &state.host.qh, ());
                state.bridge.host_syncobj_timelines.insert(tl.id(), h_tl);
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_linux_drm_syncobj_timeline_v1::WpLinuxDrmSyncobjTimelineV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wp_linux_drm_syncobj_timeline_v1::WpLinuxDrmSyncobjTimelineV1,
        request: wp_linux_drm_syncobj_timeline_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let wp_linux_drm_syncobj_timeline_v1::Request::Destroy = request
            && let Some(h_tl) = state.bridge.host_syncobj_timelines.remove(&resource.id())
        {
            h_tl.destroy();
        }
    }
}

impl Dispatch<wp_linux_drm_syncobj_surface_v1::WpLinuxDrmSyncobjSurfaceV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_linux_drm_syncobj_surface_v1::WpLinuxDrmSyncobjSurfaceV1,
        request: wp_linux_drm_syncobj_surface_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let Some(surf) = state.bridge.video_syncobj_surface.as_ref() else {
            return;
        };
        match request {
            wp_linux_drm_syncobj_surface_v1::Request::SetAcquirePoint {
                timeline,
                point_hi,
                point_lo,
            } => {
                if let Some(h_tl) = state.bridge.host_syncobj_timelines.get(&timeline.id()) {
                    surf.set_acquire_point(h_tl, point_hi, point_lo);
                }
            }
            wp_linux_drm_syncobj_surface_v1::Request::SetReleasePoint {
                timeline,
                point_hi,
                point_lo,
            } => {
                if let Some(h_tl) = state.bridge.host_syncobj_timelines.get(&timeline.id()) {
                    surf.set_release_point(h_tl, point_hi, point_lo);
                }
            }
            wp_linux_drm_syncobj_surface_v1::Request::Destroy => {
                if let Some(s) = state.bridge.video_syncobj_surface.take() {
                    s.destroy();
                }
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<wp_fifo_manager_v1::WpFifoManagerV1, ()> for AppState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wp_fifo_manager_v1::WpFifoManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wp_fifo_manager_v1::WpFifoManagerV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_fifo_manager_v1::WpFifoManagerV1,
        request: wp_fifo_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_fifo_manager_v1::Request::GetFifo { id, .. } = request {
            data_init.init(id, ());
            if state.bridge.video_fifo.is_none()
                && let Some(s) = state.host.surfaces.as_ref()
                && let Some(mgr) = s.ext.fifo_mgr.as_ref()
            {
                state.bridge.video_fifo = Some(mgr.get_fifo(&s.video_surface, &state.host.qh, ()));
            }
        }
    }
}

impl Dispatch<wp_fifo_v1::WpFifoV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_fifo_v1::WpFifoV1,
        request: wp_fifo_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_fifo_v1::Request::SetBarrier => {
                if let Some(f) = state.bridge.video_fifo.as_ref() {
                    f.set_barrier();
                }
            }
            wp_fifo_v1::Request::WaitBarrier => {
                if let Some(f) = state.bridge.video_fifo.as_ref() {
                    f.wait_barrier();
                }
            }
            wp_fifo_v1::Request::Destroy => {
                if let Some(f) = state.bridge.video_fifo.take() {
                    f.destroy();
                }
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<wp_commit_timing_manager_v1::WpCommitTimingManagerV1, ()> for AppState {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wp_commit_timing_manager_v1::WpCommitTimingManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wp_commit_timing_manager_v1::WpCommitTimingManagerV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_commit_timing_manager_v1::WpCommitTimingManagerV1,
        request: wp_commit_timing_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_commit_timing_manager_v1::Request::GetTimer { id, .. } = request {
            data_init.init(id, ());
            if state.bridge.video_commit_timer.is_none()
                && let Some(s) = state.host.surfaces.as_ref()
                && let Some(mgr) = s.ext.commit_timing_mgr.as_ref()
            {
                state.bridge.video_commit_timer =
                    Some(mgr.get_timer(&s.video_surface, &state.host.qh, ()));
            }
        }
    }
}

impl Dispatch<wp_commit_timer_v1::WpCommitTimerV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_commit_timer_v1::WpCommitTimerV1,
        request: wp_commit_timer_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_commit_timer_v1::Request::SetTimestamp {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
            } => {
                if let Some(t) = state.bridge.video_commit_timer.as_ref() {
                    t.set_timestamp(tv_sec_hi, tv_sec_lo, tv_nsec);
                }
            }
            wp_commit_timer_v1::Request::Destroy => {
                if let Some(t) = state.bridge.video_commit_timer.take() {
                    t.destroy();
                }
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<wp_color_representation_manager_v1::WpColorRepresentationManagerV1, ()>
    for AppState
{
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wp_color_representation_manager_v1::WpColorRepresentationManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let mgr = data_init.init(resource, ());
        for &mode in &state.host.cr_alpha_modes {
            if let Ok(m) = wp_color_representation_surface_v1::AlphaMode::try_from(mode) {
                mgr.supported_alpha_mode(m);
            }
        }
        for &(coeffs, range) in &state.host.cr_coeffs_ranges {
            if let (Ok(c), Ok(r)) = (
                wp_color_representation_surface_v1::Coefficients::try_from(coeffs),
                wp_color_representation_surface_v1::Range::try_from(range),
            ) {
                mgr.supported_coefficients_and_ranges(c, r);
            }
        }
        if state.host.cr_done {
            mgr.done();
        }
    }
}

impl Dispatch<wp_color_representation_manager_v1::WpColorRepresentationManagerV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_color_representation_manager_v1::WpColorRepresentationManagerV1,
        request: wp_color_representation_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_color_representation_manager_v1::Request::GetSurface { id, .. } = request {
            data_init.init(id, ());
            if state.bridge.video_cr_surface.is_none()
                && let Some(s) = state.host.surfaces.as_ref()
                && let Some(mgr) = s.ext.color_repr_mgr.as_ref()
            {
                state.bridge.video_cr_surface =
                    Some(mgr.get_surface(&s.video_surface, &state.host.qh, ()));
            }
        }
    }
}

impl Dispatch<wp_color_representation_surface_v1::WpColorRepresentationSurfaceV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_color_representation_surface_v1::WpColorRepresentationSurfaceV1,
        request: wp_color_representation_surface_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let Some(surf) = state.bridge.video_cr_surface.as_ref() else {
            return;
        };
        match request {
            wp_color_representation_surface_v1::Request::SetAlphaMode { alpha_mode } => {
                if let Ok(m) = h_crs::AlphaMode::try_from(u32::from(alpha_mode)) {
                    surf.set_alpha_mode(m);
                }
            }
            wp_color_representation_surface_v1::Request::SetCoefficientsAndRange {
                coefficients,
                range,
            } => {
                if let (Ok(c), Ok(r)) = (
                    h_crs::Coefficients::try_from(u32::from(coefficients)),
                    h_crs::Range::try_from(u32::from(range)),
                ) {
                    surf.set_coefficients_and_range(c, r);
                }
            }
            wp_color_representation_surface_v1::Request::SetChromaLocation { chroma_location } => {
                if let Ok(cl) = h_crs::ChromaLocation::try_from(u32::from(chroma_location)) {
                    surf.set_chroma_location(cl);
                }
            }
            wp_color_representation_surface_v1::Request::Destroy => {
                if let Some(s) = state.bridge.video_cr_surface.take() {
                    s.destroy();
                }
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<wp_color_manager_v1::WpColorManagerV1, ()> for AppState {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wp_color_manager_v1::WpColorManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let cm = data_init.init(resource, ());
        for &intent in &state.host.cm_intents {
            if let Ok(val) = wp_color_manager_v1::RenderIntent::try_from(intent) {
                cm.supported_intent(val);
            }
        }
        for &feat in &state.host.cm_features {
            if let Ok(val) = wp_color_manager_v1::Feature::try_from(feat) {
                cm.supported_feature(val);
            }
        }
        for &tf in &state.host.cm_tfs {
            if let Ok(val) = wp_color_manager_v1::TransferFunction::try_from(tf) {
                cm.supported_tf_named(val);
            }
        }
        for &prim in &state.host.cm_primaries {
            if let Ok(val) = wp_color_manager_v1::Primaries::try_from(prim) {
                cm.supported_primaries_named(val);
            }
        }
        if state.host.cm_done {
            cm.done();
        }
    }
}

impl Dispatch<wp_color_manager_v1::WpColorManagerV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_color_manager_v1::WpColorManagerV1,
        request: wp_color_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let Some(s) = state.host.surfaces.as_ref() else {
            return;
        };
        let Some(mgr) = s.ext.color_mgr.as_ref() else {
            return;
        };
        match request {
            wp_color_manager_v1::Request::GetOutput { id, output } => {
                let c_out = data_init.init(id, ());
                if let Some(out_gn) = state
                    .bridge
                    .client_outputs
                    .iter()
                    .find(|(_, w)| w.id() == output.id())
                    .map(|(gn, _)| *gn)
                    && let Some(h_out) = state
                        .host
                        .outputs
                        .iter()
                        .find(|o| o.global_name == out_gn)
                        .and_then(|o| o.output.as_ref())
                {
                    let ho = mgr.get_output(h_out, &state.host.qh, ());
                    state
                        .bridge
                        .client_cm_outputs
                        .insert(c_out.id(), ho.clone());
                    state.bridge.host_cm_outputs.insert(ho.id(), c_out);
                }
            }
            wp_color_manager_v1::Request::GetSurface { id, .. } => {
                data_init.init(id, ());
                if state.bridge.video_cm_surface.is_none() {
                    state.bridge.video_cm_surface =
                        Some(mgr.get_surface(&s.video_surface, &state.host.qh, ()));
                }
            }
            wp_color_manager_v1::Request::GetSurfaceFeedback { id, .. } => {
                let c_fb = data_init.init(id, ());
                let h_fb = mgr.get_surface_feedback(&s.video_surface, &state.host.qh, ());
                state
                    .bridge
                    .client_cm_feedbacks
                    .insert(c_fb.id(), h_fb.clone());
                state.bridge.host_cm_feedbacks.insert(h_fb.id(), c_fb);
            }
            wp_color_manager_v1::Request::CreateIccCreator { obj } => {
                let c = data_init.init(obj, ());
                let hc = mgr.create_icc_creator(&state.host.qh, ());
                state.bridge.client_icc_creators.insert(c.id(), hc);
            }
            wp_color_manager_v1::Request::CreateParametricCreator { obj } => {
                let c = data_init.init(obj, ());
                let hc = mgr.create_parametric_creator(&state.host.qh, ());
                state.bridge.client_param_creators.insert(c.id(), hc);
            }
            wp_color_manager_v1::Request::CreateWindowsScrgb { image_description } => {
                let c_desc = data_init.init(image_description, ());
                let h_desc = mgr.create_windows_scrgb(&state.host.qh, ());
                register_desc_pair(state, c_desc, h_desc);
            }
            wp_color_manager_v1::Request::CreateWindowsBt2100 { image_description } => {
                let c_desc = data_init.init(image_description, ());
                let h_desc = mgr.create_windows_bt2100(&state.host.qh, ());
                register_desc_pair(state, c_desc, h_desc);
            }
            wp_color_manager_v1::Request::GetImageDescription {
                image_description,
                reference,
            } => {
                let c_desc = data_init.init(image_description, ());
                if let Some(h_ref) = state.bridge.client_img_refs_by_id.get(&reference.id()) {
                    let h_desc = mgr.get_image_description(h_ref, &state.host.qh, ());
                    register_desc_pair(state, c_desc, h_desc);
                }
            }
            _ => {}
        }
    }
}

fn register_desc_pair(
    state: &mut AppState,
    c_desc: wp_image_description_v1::WpImageDescriptionV1,
    h_desc: h_desc::WpImageDescriptionV1,
) {
    state
        .bridge
        .client_img_descs_by_id
        .insert(c_desc.id(), h_desc.clone());
    state
        .bridge
        .host_img_descs
        .insert(h_desc.id(), (h_desc, c_desc));
}

impl Dispatch<wp_color_management_output_v1::WpColorManagementOutputV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wp_color_management_output_v1::WpColorManagementOutputV1,
        request: wp_color_management_output_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_color_management_output_v1::Request::GetImageDescription { image_description } => {
                let c_desc = data_init.init(image_description, ());
                if let Some(h_out) = state.bridge.client_cm_outputs.get(&resource.id()) {
                    let h_desc = h_out.get_image_description(&state.host.qh, ());
                    register_desc_pair(state, c_desc, h_desc);
                }
            }
            wp_color_management_output_v1::Request::Destroy => {
                if let Some(h_out) = state.bridge.client_cm_outputs.remove(&resource.id()) {
                    state.bridge.host_cm_outputs.remove(&h_out.id());
                    h_out.destroy();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_color_management_surface_v1::WpColorManagementSurfaceV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wp_color_management_surface_v1::WpColorManagementSurfaceV1,
        request: wp_color_management_surface_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let Some(surf) = state.bridge.video_cm_surface.as_ref() else {
            return;
        };
        match request {
            wp_color_management_surface_v1::Request::SetImageDescription {
                image_description,
                render_intent,
            } => {
                if let Some(h_desc) = state
                    .bridge
                    .client_img_descs_by_id
                    .get(&image_description.id())
                    && let Ok(ri) = h_cm::RenderIntent::try_from(u32::from(render_intent))
                {
                    surf.set_image_description(h_desc, ri);
                }
            }
            wp_color_management_surface_v1::Request::UnsetImageDescription => {
                surf.unset_image_description();
            }
            wp_color_management_surface_v1::Request::Destroy => {
                if let Some(s) = state.bridge.video_cm_surface.take() {
                    s.destroy();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1, ()>
    for AppState
{
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1,
        request: wp_color_management_surface_feedback_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_color_management_surface_feedback_v1::Request::GetPreferred {
                image_description,
            } => {
                let c_desc = data_init.init(image_description, ());
                if let Some(h_fb) = state.bridge.client_cm_feedbacks.get(&resource.id()) {
                    let h_desc = h_fb.get_preferred(&state.host.qh, ());
                    register_desc_pair(state, c_desc, h_desc);
                }
            }
            wp_color_management_surface_feedback_v1::Request::GetPreferredParametric {
                image_description,
            } => {
                let c_desc = data_init.init(image_description, ());
                if let Some(h_fb) = state.bridge.client_cm_feedbacks.get(&resource.id()) {
                    let h_desc = h_fb.get_preferred_parametric(&state.host.qh, ());
                    register_desc_pair(state, c_desc, h_desc);
                }
            }
            wp_color_management_surface_feedback_v1::Request::Destroy => {
                if let Some(h_fb) = state.bridge.client_cm_feedbacks.remove(&resource.id()) {
                    state.bridge.host_cm_feedbacks.remove(&h_fb.id());
                    h_fb.destroy();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_image_description_creator_icc_v1::WpImageDescriptionCreatorIccV1, ()>
    for AppState
{
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wp_image_description_creator_icc_v1::WpImageDescriptionCreatorIccV1,
        request: wp_image_description_creator_icc_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_image_description_creator_icc_v1::Request::SetIccFile {
                icc_profile,
                offset,
                length,
            } => {
                if let Some(hc) = state.bridge.client_icc_creators.get(&resource.id()) {
                    hc.set_icc_file(icc_profile.as_fd(), offset, length);
                }
            }
            wp_image_description_creator_icc_v1::Request::Create { image_description } => {
                let c_desc = data_init.init(image_description, ());
                if let Some(hc) = state.bridge.client_icc_creators.remove(&resource.id()) {
                    let h_desc = hc.create(&state.host.qh, ());
                    register_desc_pair(state, c_desc, h_desc);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1, ()>
    for AppState
{
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1,
        request: wp_image_description_creator_params_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_image_description_creator_params_v1::Request::Create { image_description } => {
                let c_desc = data_init.init(image_description, ());
                if let Some(hc) = state.bridge.client_param_creators.remove(&resource.id()) {
                    let h_desc = hc.create(&state.host.qh, ());
                    register_desc_pair(state, c_desc, h_desc);
                }
            }
            wp_image_description_creator_params_v1::Request::SetTfNamed { tf } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id())
                    && let Ok(val) = h_cm::TransferFunction::try_from(u32::from(tf))
                {
                    hc.set_tf_named(val);
                }
            }
            wp_image_description_creator_params_v1::Request::SetTfPower { eexp } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id()) {
                    hc.set_tf_power(eexp);
                }
            }
            wp_image_description_creator_params_v1::Request::SetPrimariesNamed { primaries } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id())
                    && let Ok(val) = h_cm::Primaries::try_from(u32::from(primaries))
                {
                    hc.set_primaries_named(val);
                }
            }
            wp_image_description_creator_params_v1::Request::SetPrimaries {
                r_x,
                r_y,
                g_x,
                g_y,
                b_x,
                b_y,
                w_x,
                w_y,
            } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id()) {
                    hc.set_primaries(r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y);
                }
            }
            wp_image_description_creator_params_v1::Request::SetLuminances {
                min_lum,
                max_lum,
                reference_lum,
            } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id()) {
                    hc.set_luminances(min_lum, max_lum, reference_lum);
                }
            }
            wp_image_description_creator_params_v1::Request::SetMasteringDisplayPrimaries {
                r_x,
                r_y,
                g_x,
                g_y,
                b_x,
                b_y,
                w_x,
                w_y,
            } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id()) {
                    hc.set_mastering_display_primaries(r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y);
                }
            }
            wp_image_description_creator_params_v1::Request::SetMasteringLuminance {
                min_lum,
                max_lum,
            } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id()) {
                    hc.set_mastering_luminance(min_lum, max_lum);
                }
            }
            wp_image_description_creator_params_v1::Request::SetMaxCll { max_cll } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id()) {
                    hc.set_max_cll(max_cll);
                }
            }
            wp_image_description_creator_params_v1::Request::SetMaxFall { max_fall } => {
                if let Some(hc) = state.bridge.client_param_creators.get(&resource.id()) {
                    hc.set_max_fall(max_fall);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_image_description_v1::WpImageDescriptionV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wp_image_description_v1::WpImageDescriptionV1,
        request: wp_image_description_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wp_image_description_v1::Request::GetInformation { information } => {
                let c_info = data_init.init(information, ());
                if let Some(h_desc) = state.bridge.client_img_descs_by_id.get(&resource.id()) {
                    let h_info = h_desc.get_information(&state.host.qh, ());
                    state.bridge.host_img_infos.insert(h_info.id(), c_info);
                }
            }
            wp_image_description_v1::Request::Destroy => {
                if let Some(h_desc) = state.bridge.client_img_descs_by_id.remove(&resource.id()) {
                    state.bridge.host_img_descs.remove(&h_desc.id());
                    h_desc.destroy();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_image_description_info_v1::WpImageDescriptionInfoV1, ()> for AppState {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wp_image_description_info_v1::WpImageDescriptionInfoV1,
        _: wp_image_description_info_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<wp_image_description_reference_v1::WpImageDescriptionReferenceV1, ()> for AppState {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wp_image_description_reference_v1::WpImageDescriptionReferenceV1,
        request: wp_image_description_reference_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let wp_image_description_reference_v1::Request::Destroy = request
            && let Some(h_ref) = state.bridge.client_img_refs_by_id.remove(&resource.id())
        {
            h_ref.destroy();
        }
    }
}
