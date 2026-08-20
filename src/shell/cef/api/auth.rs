use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["auth", "connect"] if request.is("POST") => auth_connect(services, request),
        ["auth", "login"] if request.is("POST") => auth_login(services, request),
        ["auth", "quickconnect", "start"] if request.is("POST") => {
            quick_connect_start(services, request)
        }
        ["auth", "quickconnect", "poll"] if request.is("POST") => {
            quick_connect_poll(services, request)
        }
        ["auth", "logout"] if request.is("POST") => auth_logout(services, request),
        _ => return None,
    };
    Some(response)
}

fn auth_connect(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let server = body["server"].as_str().unwrap_or_default();
    match services.session.connect(server) {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn auth_login(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let result = services.session.login(
        body["server"].as_str().unwrap_or_default(),
        body["username"].as_str().unwrap_or_default(),
        body["password"].as_str().unwrap_or_default(),
    );
    match result {
        Ok(_) => {
            // Signing in as somebody else must not inherit their Seerr link.
            services.seerr.revalidate();
            services.companion.clear();
            if let Err(error) = services.companion.probe(true) {
                services.session.note_error(&error);
                tracing::debug!(target: "companion", "post-login probe failed: {error}");
            }
            services.sync.request();
            status(services)
        }
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn quick_connect_start(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    match services
        .session
        .quick_connect_start(body["server"].as_str().unwrap_or_default())
    {
        Ok(value) => ApiResponse::ok(value),
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn quick_connect_poll(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let body = request.json();
    let result = services.session.quick_connect_poll(
        body["server"].as_str().unwrap_or_default(),
        body["secret"].as_str().unwrap_or_default(),
    );
    match result {
        Ok(value) => {
            if value["authenticated"] == json!(true) {
                services.seerr.revalidate();
                services.companion.clear();
                if let Err(error) = services.companion.probe(true) {
                    services.session.note_error(&error);
                    tracing::debug!(target: "companion", "post-login probe failed: {error}");
                }
                services.sync.request();
            }
            ApiResponse::ok(value)
        }
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn auth_logout(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let forget = request.json()["forgetLibrary"].as_bool().unwrap_or(false);
    services.session.logout(forget);
    services.companion.clear();
    // The Seerr link belongs to the account that just went away. Every read
    // path re-checks that anyway, but doing it here means a signed-out machine
    // keeps no Seerr cookie on disk.
    services.seerr.revalidate();
    status(services)
}
