use serde_json::{Value, json};

use super::SeerrSession;
use crate::seerr::api::error::SeerrError;
use crate::seerr::api::model::{
    self as model, Capabilities, DownloadService, DownloadServiceDetail, MediaRequest, RequestPage,
    SeerrUser,
};

/// An advanced Seerr request pins the title to one linked Radarr/Sonarr
/// destination and one quality profile owned by that destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestProfileSelection {
    pub server_id: i64,
    pub profile_id: i64,
}

impl SeerrSession {
    /// The quality profiles on the linked Radarr or Sonarr destinations that
    /// can receive this request. Seerr exposes these only as an advanced
    /// request choice, so the user's permission is checked before returning
    /// any destination metadata.
    pub fn request_options(&self, media_type: &str, is_4k: bool) -> Result<Value, SeerrError> {
        let service = request_service(media_type)?;
        let user: SeerrUser = self.call(|client| client.get_json("auth/me", &[]))?;
        let capabilities = Capabilities::derive(user.permissions, true, true);
        if !capabilities.advanced_request {
            return Err(SeerrError::PermissionDenied);
        }

        let path = format!("service/{service}");
        let mut servers: Vec<DownloadService> = self.call(|client| client.get_json(&path, &[]))?;
        servers.retain(|server| server.id >= 0 && server.is_4k == is_4k);
        servers.sort_by_key(|server| (!server.is_default, server.name.to_lowercase()));

        let mut destinations = Vec::with_capacity(servers.len());
        for server in servers {
            let path = format!("service/{service}/{}", server.id);
            let mut detail: DownloadServiceDetail =
                self.call(|client| client.get_json(&path, &[]))?;
            detail
                .profiles
                .retain(|profile| profile.id > 0 && !profile.name.trim().is_empty());
            detail
                .profiles
                .sort_by_key(|profile| profile.name.to_lowercase());
            destinations.push(json!({
                "id": server.id,
                "name": server.name,
                "isDefault": server.is_default,
                "profiles": detail.profiles.iter().map(|profile| json!({
                    "id": profile.id,
                    "name": profile.name,
                    "isDefault": profile.id == server.active_profile_id,
                })).collect::<Vec<_>>(),
            }));
        }

        Ok(json!({ "destinations": destinations }))
    }

    // ----------------------------------------------------------------- writes

    /// Asks Seerr for a title. Never retried. See
    /// [`SeerrClient`](crate::seerr::api::client::SeerrClient), because an
    /// uncertain outcome must not become a second request.
    ///
    /// `seasons` is what makes a partial series requestable one season at a
    /// time; `None` on a series means "everything Seerr does not already have",
    /// which is Seerr's own `all`.
    pub fn create_request(
        &self,
        media_type: &str,
        tmdb_id: i64,
        seasons: Option<Vec<i64>>,
        is_4k: bool,
        profile: Option<RequestProfileSelection>,
    ) -> Result<Value, SeerrError> {
        if model::library_kind(media_type).is_none() {
            return Err(SeerrError::Unusable(format!(
                "{media_type} is not a kind of title that can be requested"
            )));
        }
        if tmdb_id <= 0 {
            return Err(SeerrError::Unusable("that is not a TMDB id".to_string()));
        }
        let mut body = json!({
            "mediaType": media_type,
            "mediaId": tmdb_id,
            "is4k": is_4k,
        });
        if let Some(profile) = profile {
            if profile.server_id < 0 || profile.profile_id <= 0 {
                return Err(SeerrError::Unusable(
                    "the download destination must be non-negative and the quality profile must be positive"
                        .to_string(),
                ));
            }
            let user: SeerrUser = self.call(|client| client.get_json("auth/me", &[]))?;
            if !Capabilities::derive(user.permissions, true, true).advanced_request {
                return Err(SeerrError::PermissionDenied);
            }
            body["serverId"] = json!(profile.server_id);
            body["profileId"] = json!(profile.profile_id);
        }
        if media_type == model::TV {
            body["seasons"] = match seasons {
                Some(seasons) if !seasons.is_empty() => json!(seasons),
                // Seerr expands this to whatever it does not already have,
                // which is what an unqualified "request this show" means.
                _ => json!("all"),
            };
        }
        let created: MediaRequest = self.call(|client| client.post_json("request", &body))?;
        tracing::info!(
            target: "seerr.session",
            media_type,
            tmdb_id,
            status = model::request_status_name(created.status),
            "requested a title through Seerr"
        );
        Ok(self.request_json(&created))
    }

    /// The user's own requests. Deliberately never the household's: Seerr shows
    /// everyone's to an administrator, and this is a personal view.
    pub fn requests(&self, take: i64, skip: i64, filter: &str) -> Result<Value, SeerrError> {
        self.revalidate();
        let state = self.read();
        if state.base_url.is_none() {
            return Err(SeerrError::NotConfigured);
        }
        // Without an account there is nobody to scope the list to, and an
        // unscoped one would be the household's.
        let Some(user_id) = state.user_id else {
            return Err(SeerrError::Unauthorized);
        };
        let take = take.clamp(1, 100);
        let skip = skip.max(0);
        let filter = match filter {
            "all" | "pending" | "approved" | "processing" | "available" | "failed" => filter,
            _ => "all",
        }
        .to_string();
        let page: RequestPage = self.call(|client| {
            client.get_json(
                "request",
                &[
                    ("take", take.to_string()),
                    ("skip", skip.to_string()),
                    ("filter", filter),
                    ("sort", "added".to_string()),
                    ("requestedBy", user_id.to_string()),
                ],
            )
        })?;

        let results = page
            .results
            .iter()
            .map(|request| self.request_json(request))
            .collect::<Vec<_>>();
        Ok(json!({
            "page": page.page_info.page,
            "totalPages": page.page_info.pages,
            "totalResults": page.page_info.results,
            "results": results,
        }))
    }

    /// Cancels one of the user's own pending requests.
    ///
    /// A refusal here is the case the 401 disambiguation exists for: Seerr
    /// answers 401 — not 403 — when the session is valid but the request is not
    /// the user's to cancel, and that must not read as a lapsed session.
    pub fn cancel_request(&self, request_id: i64) -> Result<Value, SeerrError> {
        if request_id <= 0 {
            return Err(SeerrError::Unusable("that is not a request id".to_string()));
        }
        let path = format!("request/{request_id}");
        self.call(|client| client.delete(&path))?;
        tracing::info!(target: "seerr.session", request_id, "cancelled a Seerr request");
        Ok(json!({ "cancelled": true, "id": request_id }))
    }

    /// One request in the shape the UI renders. The title is deliberately
    /// absent: Seerr's request rows reference a TMDB id and nothing else, and
    /// resolving each one here would be a blocking fan-out on this thread.
    fn request_json(&self, request: &MediaRequest) -> Value {
        let media = request.media.clone().unwrap_or_default();
        let media_type = if request.media_type.is_empty() {
            media.media_type.clone().unwrap_or_default()
        } else {
            request.media_type.clone()
        };
        let tmdb_id = media.tmdb_id;
        let library_item_id = tmdb_id.and_then(|tmdb_id| {
            self.local_ids(&media_type, &[tmdb_id.to_string()])
                .remove(&tmdb_id.to_string())
        });
        json!({
            "id": request.id,
            "status": model::request_status_name(request.status),
            "mediaType": media_type,
            "tmdbId": tmdb_id,
            "is4k": request.is4k,
            "createdAt": request.created_at,
            "updatedAt": request.updated_at,
            "mediaStatus": model::status_name(if request.is4k {
                media.status_4k
            } else {
                media.status
            }),
            "seasons": request
                .seasons
                .iter()
                .map(|season| season.season_number)
                .collect::<Vec<_>>(),
            "libraryItemId": library_item_id,
        })
    }
}

fn request_service(media_type: &str) -> Result<&'static str, SeerrError> {
    match media_type {
        model::MOVIE => Ok("radarr"),
        model::TV => Ok("sonarr"),
        _ => Err(SeerrError::Unusable(format!(
            "{media_type} is not a kind of title that can be requested"
        ))),
    }
}
