use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["shell", "file-picker"] if request.is("POST") => shell_file_picker(services, request),
        ["shell", "mpv", "install"] if request.is("POST") => shell_install_mpv(services, request),
        ["shell", "mpv", "help"] if request.is("POST") => shell_mpv_help(),
        _ => return None,
    };
    Some(response)
}

fn shell_file_picker(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let request_id = match shell_request_id(body.get("requestId").and_then(Value::as_str)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let target = match body.get("target").and_then(Value::as_str) {
        Some("mpv") => ShellFilePickerTarget::Mpv,
        Some("mpchc") => ShellFilePickerTarget::Mpchc,
        _ => return ApiResponse::error(400, "target must be mpv or mpchc"),
    };
    match services.shell.request(ShellRequest::FilePicker {
        request_id: request_id.clone(),
        target,
    }) {
        Ok(()) => ApiResponse::ok(json!({ "requestId": request_id, "queued": true })),
        Err(error) => ApiResponse::error(503, error),
    }
}

fn shell_install_mpv(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    if !player_setup::supported() {
        return ApiResponse::error(
            409,
            "automatic mpv installation is not available on this platform",
        );
    }
    let body = request.json();
    let request_id = match shell_request_id(body.get("requestId").and_then(Value::as_str)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    match services.shell.request(ShellRequest::InstallMpv {
        request_id: request_id.clone(),
    }) {
        Ok(()) => ApiResponse::ok(json!({ "requestId": request_id, "queued": true })),
        Err(error) => ApiResponse::error(503, error),
    }
}

fn shell_mpv_help() -> ApiResponse {
    super::super::bridge::open_external_link(player_setup::MPV_HELP_URL);
    ApiResponse::ok(json!({ "opened": true }))
}

fn shell_request_id(value: Option<&str>) -> Result<String, ApiResponse> {
    let value = value.unwrap_or_default().trim();
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
