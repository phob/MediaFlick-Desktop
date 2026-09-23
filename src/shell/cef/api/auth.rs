use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
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

#[derive(Deserialize)]
struct ServerBody {
    server: String,
}

#[derive(Deserialize)]
struct LoginBody {
    server: String,
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct QuickConnectPollBody {
    server: String,
    secret: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogoutBody {
    #[serde(default)]
    forget_library: bool,
}

fn auth_connect(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.body::<ServerBody>()?;
    match services.session.connect(&body.server) {
        Ok(value) => Ok(ApiResponse::ok(value)),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn auth_login(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.body::<LoginBody>()?;
    let result = services
        .session
        .login(&body.server, &body.username, &body.password);
    match result {
        Ok(_) => {
            remember_server_url(services);
            activate_account_preferences(services)?;
            services.companion.clear();
            if let Err(error) = services.companion.probe(true) {
                tracing::debug!(target: "companion", "post-login probe failed: {error}");
            }
            services.sync.request();
            crate::collections::scheduler::request_run(services.clone());
            status(services)
        }
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn quick_connect_start(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.body::<ServerBody>()?;
    match services.session.quick_connect_start(&body.server) {
        Ok(value) => Ok(ApiResponse::ok(value)),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn quick_connect_poll(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.body::<QuickConnectPollBody>()?;
    let result = services
        .session
        .quick_connect_poll(&body.server, &body.secret);
    match result {
        Ok(value) => {
            if value["authenticated"] == json!(true) {
                remember_server_url(services);
                activate_account_preferences(services)?;
                services.companion.clear();
                if let Err(error) = services.companion.probe(true) {
                    tracing::debug!(target: "companion", "post-login probe failed: {error}");
                }
                services.sync.request();
                crate::collections::scheduler::request_run(services.clone());
            }
            Ok(ApiResponse::ok(value))
        }
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn auth_logout(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let body = request.body::<LogoutBody>()?;
    if let Err(error) = services.session.logout(body.forget_library) {
        tracing::error!("could not clear the local session after logout: {error}");
        return Err(ApiResponse::error(500, "could not clear the local session"));
    }
    if let Err(error) = services.preferences.activate_account(None) {
        tracing::error!("could not clear account preferences after logout: {error}");
        return Err(ApiResponse::error(
            500,
            "could not clear account preferences",
        ));
    }
    services.companion.clear();
    status(services)
}

/// Keeps `settings.json` in step with the signed-in server, so the dashboard
/// action and upgrades from older releases keep working.
fn remember_server_url(services: &Services) {
    let Some(server_url) = services.session.server_url() else {
        return;
    };
    if services.preferences.snapshot().jellyfin_url.as_deref() == Some(server_url.as_str()) {
        return;
    }
    if let Err(error) = services.preferences.set_server_url(server_url) {
        tracing::warn!(target: "jellyfin.session", "failed to save the server URL: {error}");
    }
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
