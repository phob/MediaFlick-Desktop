use crate::jellyfin::session::{QuickConnectPoll, QuickConnectStart, ServerInfo};

use super::*;

routes! {
    |services, request|
    POST ["auth", "connect"] (body: ServerBody) -> ServerInfo = auth_connect(services, body),
    POST ["auth", "login"] (body: LoginBody) -> AppStatus = auth_login(services, body),
    POST ["auth", "quickconnect", "start"] (body: ServerBody) -> QuickConnectStart =
        quick_connect_start(services, body),
    POST ["auth", "quickconnect", "poll"] (body: QuickConnectPollBody) -> QuickConnectPoll =
        quick_connect_poll(services, body),
    POST ["auth", "logout"] (body: LogoutBody) -> AppStatus = auth_logout(services, body),
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct ServerBody {
    server: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct LoginBody {
    server: String,
    username: String,
    password: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct QuickConnectPollBody {
    server: String,
    secret: String,
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct LogoutBody {
    #[serde(default)]
    #[ts(optional = nullable)]
    forget_library: bool,
}

fn auth_connect(
    services: &Arc<Services>,
    body: Body<'_, ServerBody>,
) -> Result<ServerInfo, ApiResponse> {
    let body = body.decode()?;
    match services.session.connect(&body.server) {
        Ok(server) => Ok(server),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn auth_login(
    services: &Arc<Services>,
    body: Body<'_, LoginBody>,
) -> Result<AppStatus, ApiResponse> {
    let body = body.decode()?;
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
            Ok(app_status(services))
        }
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn quick_connect_start(
    services: &Arc<Services>,
    body: Body<'_, ServerBody>,
) -> Result<QuickConnectStart, ApiResponse> {
    let body = body.decode()?;
    match services.session.quick_connect_start(&body.server) {
        Ok(start) => Ok(start),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn quick_connect_poll(
    services: &Arc<Services>,
    body: Body<'_, QuickConnectPollBody>,
) -> Result<QuickConnectPoll, ApiResponse> {
    let body = body.decode()?;
    let result = services
        .session
        .quick_connect_poll(&body.server, &body.secret);
    match result {
        Ok(poll) => {
            if poll.authenticated {
                remember_server_url(services);
                activate_account_preferences(services)?;
                services.companion.clear();
                if let Err(error) = services.companion.probe(true) {
                    tracing::debug!(target: "companion", "post-login probe failed: {error}");
                }
                services.sync.request();
                crate::collections::scheduler::request_run(services.clone());
            }
            Ok(poll)
        }
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn auth_logout(
    services: &Arc<Services>,
    body: Body<'_, LogoutBody>,
) -> Result<AppStatus, ApiResponse> {
    let body = body.decode()?;
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
    Ok(app_status(services))
}

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
