use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
    let response = match segments {
        ["shell", "window", "ready"] if request.is("POST") => shell_window_ready(services),
        ["shell", "file-picker"] if request.is("POST") => shell_file_picker(services, request),
        ["shell", "mpv", "install"] if request.is("POST") => shell_install_mpv(services, request),
        ["shell", "mpv", "help"] if request.is("POST") => shell_mpv_help(),
        _ => return None,
    };
    Some(response)
}

/// `/api/shell/window/ready`: the shell queued the request.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct ShellQueued {
    queued: bool,
}

/// A shell request the UI later matches to its event by `requestId`.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShellRequestQueued {
    request_id: String,
    queued: bool,
}

/// `/api/shell/mpv/help`: the mpv help page was handed to the browser.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct MpvHelpOpened {
    opened: bool,
}

fn shell_window_ready(services: &Arc<Services>) -> Handled {
    services.sync.release_startup_hold();
    match services.shell.request(ShellRequest::MainWindowReady) {
        Ok(()) => Ok(ApiResponse::ok(ShellQueued { queued: true })),
        Err(error) => Err(ApiResponse::error(503, error)),
    }
}

fn shell_file_picker(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let request_id = shell_request_id(request)?;
    match services.shell.request(ShellRequest::FilePicker {
        request_id: request_id.clone(),
    }) {
        Ok(()) => Ok(ApiResponse::ok(ShellRequestQueued {
            request_id,
            queued: true,
        })),
        Err(error) => Err(ApiResponse::error(503, error)),
    }
}

fn shell_install_mpv(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    if !player_setup::supported() {
        return Err(ApiResponse::error(
            409,
            "automatic mpv installation is not available on this platform",
        ));
    }
    let request_id = shell_request_id(request)?;
    match services.shell.request(ShellRequest::InstallMpv {
        request_id: request_id.clone(),
    }) {
        Ok(()) => Ok(ApiResponse::ok(ShellRequestQueued {
            request_id,
            queued: true,
        })),
        Err(error) => Err(ApiResponse::error(503, error)),
    }
}

fn shell_mpv_help() -> Handled {
    super::super::bridge::open_external_link(player_setup::MPV_HELP_URL);
    Ok(ApiResponse::ok(MpvHelpOpened { opened: true }))
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShellRequestBody {
    request_id: String,
}

fn shell_request_id(request: &ApiRequest) -> Result<String, ApiResponse> {
    let body = request.body::<ShellRequestBody>()?;
    let value = body.request_id.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ApiResponse::error(
            400,
            "requestId must be a short URL-safe identifier",
        ));
    }
    Ok(value.to_string())
}
