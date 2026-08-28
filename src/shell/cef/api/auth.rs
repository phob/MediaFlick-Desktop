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
            if let Err(response) = activate_account_preferences(services) {
                return response;
            }
            services.companion.clear();
            if let Err(error) = services.companion.probe(true) {
                tracing::debug!(target: "companion", "post-login probe failed: {error}");
            }
            services.sync.request();
            crate::collections::scheduler::request_run(services.clone());
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
                if let Err(response) = activate_account_preferences(services) {
                    return response;
                }
                services.companion.clear();
                if let Err(error) = services.companion.probe(true) {
                    tracing::debug!(target: "companion", "post-login probe failed: {error}");
                }
                services.sync.request();
                crate::collections::scheduler::request_run(services.clone());
            }
            ApiResponse::ok(value)
        }
        Err(error) => ApiResponse::from_api_error(&error),
    }
}

fn auth_logout(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let forget = request.json()["forgetLibrary"].as_bool().unwrap_or(false);
    if let Err(error) = services.session.logout(forget) {
        tracing::error!("could not clear the local session after logout: {error}");
        return ApiResponse::error(500, "could not clear the local session");
    }
    if let Err(error) = services.preferences.activate_account(None) {
        tracing::error!("could not clear account preferences after logout: {error}");
        return ApiResponse::error(500, "could not clear account preferences");
    }
    services.companion.clear();
    status(services)
}

fn activate_account_preferences(services: &Arc<Services>) -> Result<(), ApiResponse> {
    let Some(account) = services.session.account_key() else {
        tracing::error!("authenticated Jellyfin session has no stable account identity");
        return Err(ApiResponse::error(
            500,
            "the signed-in account has no stable identity",
        ));
    };
    services
        .preferences
        .activate_account(Some(account))
        .map(|_| ())
        .map_err(|error| {
            tracing::error!("could not activate account preferences: {error}");
            ApiResponse::error(500, "could not load account preferences")
        })
}
