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

fn shell_window_ready(services: &Arc<Services>) -> Handled {
    services.sync.release_startup_hold();
    match services.shell.request(ShellRequest::MainWindowReady) {
        Ok(()) => Ok(ApiResponse::ok(json!({ "queued": true }))),
        Err(error) => Err(ApiResponse::error(503, error)),
    }
}

fn shell_file_picker(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let request_id = shell_request_id(request)?;
    match services.shell.request(ShellRequest::FilePicker {
        request_id: request_id.clone(),
    }) {
        Ok(()) => Ok(ApiResponse::ok(
            json!({ "requestId": request_id, "queued": true }),
        )),
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
        Ok(()) => Ok(ApiResponse::ok(
            json!({ "requestId": request_id, "queued": true }),
        )),
        Err(error) => Err(ApiResponse::error(503, error)),
    }
}

fn shell_mpv_help() -> Handled {
    super::super::bridge::open_external_link(player_setup::MPV_HELP_URL);
    Ok(ApiResponse::ok(json!({ "opened": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShellRequestBody {
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
