use super::*;

routes! {
    |services, request|
    POST ["shell", "window", "ready"] -> ShellQueued = shell_window_ready(services),
    POST ["shell", "file-picker"] (body: ShellRequestBody) -> ShellRequestQueued =
        shell_file_picker(services, body),
    POST ["shell", "mpv", "install"] (body: ShellRequestBody) -> ShellRequestQueued =
        shell_install_mpv(services, body),
    POST ["shell", "mpv", "help"] -> MpvHelpOpened = Ok(shell_mpv_help()),
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

fn shell_window_ready(services: &Arc<Services>) -> Result<ShellQueued, ApiResponse> {
    services.sync.release_startup_hold();
    match services.shell.request(ShellRequest::MainWindowReady) {
        Ok(()) => Ok(ShellQueued { queued: true }),
        Err(error) => Err(ApiResponse::error(503, error)),
    }
}

fn shell_file_picker(
    services: &Arc<Services>,
    body: Body<'_, ShellRequestBody>,
) -> Result<ShellRequestQueued, ApiResponse> {
    let request_id = shell_request_id(body)?;
    match services.shell.request(ShellRequest::FilePicker {
        request_id: request_id.clone(),
    }) {
        Ok(()) => Ok(ShellRequestQueued {
            request_id,
            queued: true,
        }),
        Err(error) => Err(ApiResponse::error(503, error)),
    }
}

fn shell_install_mpv(
    services: &Arc<Services>,
    body: Body<'_, ShellRequestBody>,
) -> Result<ShellRequestQueued, ApiResponse> {
    if !player_setup::supported() {
        return Err(ApiResponse::error(
            409,
            "automatic mpv installation is not available on this platform",
        ));
    }
    let request_id = shell_request_id(body)?;
    match services.shell.request(ShellRequest::InstallMpv {
        request_id: request_id.clone(),
    }) {
        Ok(()) => Ok(ShellRequestQueued {
            request_id,
            queued: true,
        }),
        Err(error) => Err(ApiResponse::error(503, error)),
    }
}

fn shell_mpv_help() -> MpvHelpOpened {
    super::super::bridge::open_external_link(player_setup::MPV_HELP_URL);
    MpvHelpOpened { opened: true }
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShellRequestBody {
    request_id: String,
}

fn shell_request_id(body: Body<'_, ShellRequestBody>) -> Result<String, ApiResponse> {
    let body = body.decode()?;
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
