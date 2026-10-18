use crate::collections::{CollectionProfile, ProviderResult};

use super::*;

mod browse;
mod profiles;

pub(super) use browse::{
    CollectionProfileDetail, CollectionTitle, FranchiseCollection, FranchiseCollectionsIndex,
    MovieCollection,
};
pub(super) use profiles::{
    CollectionProfilesIndex, CollectionSettings, CollectionTemplates, CommittedProfile,
    ProfileDeleted, ProfileDraft, ProfileEdit, ReorderBody, SettingsPatch,
};

routes! {
    |services, request|
    GET ["collections", "settings"] -> CollectionSettings = profiles::settings(services, false),
    POST ["collections", "settings", "reprobe"] -> CollectionSettings =
        profiles::settings(services, true),
    PATCH ["collections", "settings"] (body: SettingsPatch) -> CollectionSettings =
        profiles::patch_settings(services, body),
    GET ["collections", "templates"] -> CollectionTemplates = profiles::templates(services),
    POST ["collections", "mdblist", "search"] (body: PublicListSearchBody) -> Relay =
        search_public_lists(services, body),
    POST ["collections", "mdblist", "validate"] (body: PublicListValidateBody) -> Relay =
        validate_public_list(services, body),
    GET ["collections", "provider-artwork"] -> Binary = provider_artwork(services, request),
    POST ["collections", "artwork"] (body: Bytes) -> UploadedArtwork =
        upload_artwork(services, body),
    GET ["collections", "artwork", artwork_id] -> Binary =
        custom_artwork(services, &percent_decode(artwork_id)),
    POST ["collections", "preview"] (body: ProfileDraft) -> ProviderResult =
        profiles::preview(services, body),
    GET ["collections", "profiles"] -> CollectionProfilesIndex = profiles::list(services),
    POST ["collections", "profiles"] (body: ProfileDraft) -> CommittedProfile =
        profiles::create(services, body),
    PUT ["collections", "profiles", "order"] (body: ReorderBody) -> CollectionProfilesIndex =
        profiles::reorder(services, body),
    GET ["collections", "profiles", profile_id] -> CollectionProfile =
        profiles::read(services, &percent_decode(profile_id)),
    PATCH ["collections", "profiles", profile_id] (body: ProfileDraft) -> ProfileEdit =
        profiles::edit(services, &percent_decode(profile_id), body),
    DELETE ["collections", "profiles", profile_id] -> ProfileDeleted =
        profiles::delete(services, &percent_decode(profile_id)),
    POST ["collections", "profiles", profile_id, "refresh"] -> CommittedProfile =
        profiles::refresh(services, &percent_decode(profile_id)),
    GET ["collections"] -> CollectionSettings = browse::redirect_state(services),
    GET ["collections", "mine"] -> CollectionProfilesIndex = browse::mine(services),
    GET ["collections", "mine", profile_id] -> CollectionProfileDetail =
        browse::profile_detail(services, &percent_decode(profile_id)),
    GET ["collections", "franchises"] -> FranchiseCollectionsIndex =
        browse::franchises(services, request),
    GET ["collections", "franchises", collection_id] -> FranchiseCollection =
        browse::franchise_detail(services, &percent_decode(collection_id), request),
    GET ["collections", "jellyfin"] -> JellyfinCollections = jellyfin_index(services),
    GET ["collections", "jellyfin", boxset_id] -> JellyfinCollectionDetail =
        jellyfin_detail(services, &percent_decode(boxset_id)),
    DELETE ["collections", "local-account"] (body: ConfirmationBody) -> AppStatus =
        delete_local_account(services, body),
    GET ["collections", "movie", tmdb_id] -> MovieCollection =
        browse::movie_franchise(services, &percent_decode(tmdb_id)),
    GET ["collections", "title", media_type, tmdb_id] -> CollectionTitle =
        browse::title(services, &percent_decode(media_type), &percent_decode(tmdb_id)),
}

fn active_scope(services: &Arc<Services>) -> Result<SessionScope, ApiResponse> {
    services
        .session
        .scope()
        .map_err(|_| ApiResponse::error(401, "sign in to use collections"))
}

fn active_account(services: &Arc<Services>) -> Result<AccountKey, ApiResponse> {
    active_scope(services).map(|scope| scope.account().clone())
}

/// `/api/collections/jellyfin`: the server's own box sets.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct JellyfinCollections {
    collections: Vec<JellyfinCollectionSummary>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct JellyfinCollectionSummary {
    id: String,
    name: String,
    primary_image_tag: Option<String>,
    backdrop_image_tag: Option<String>,
    item_count: Option<i64>,
}

/// `/api/collections/jellyfin/{id}`: one box set and its children.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct JellyfinCollectionDetail {
    id: String,
    name: String,
    primary_image_tag: Option<String>,
    backdrop_image_tag: Option<String>,
    items: Vec<ItemSummary>,
    total_record_count: i64,
}

/// `/api/collections/artwork`: the staged custom poster's id.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct UploadedArtwork {
    id: String,
}

fn jellyfin_index(services: &Arc<Services>) -> Result<JellyfinCollections, ApiResponse> {
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_box_sets(client, user_id) {
        Ok(response) => Ok(JellyfinCollections {
            collections: response
                .items
                .iter()
                .map(|item| JellyfinCollectionSummary {
                    id: item.id.clone(),
                    name: item.display_name().to_string(),
                    primary_image_tag: item.primary_image_tag().map(str::to_string),
                    backdrop_image_tag: item.backdrop_image_tags.first().cloned(),
                    item_count: item.child_count,
                })
                .collect(),
        }),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn jellyfin_detail(
    services: &Arc<Services>,
    id: &str,
) -> Result<JellyfinCollectionDetail, ApiResponse> {
    if id.is_empty() {
        return Err(ApiResponse::error(
            400,
            "that is not a Jellyfin collection id",
        ));
    }
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    let set = match items::fetch_item(client, user_id, id) {
        Ok(Some(item)) => item,
        Ok(None) => {
            return Err(ApiResponse::error(
                404,
                "that Jellyfin collection does not exist",
            ));
        }
        Err(error) => return Err(scoped_failure(services, &scope, &error)),
    };
    match items::fetch_box_set_children(client, user_id, id) {
        Ok(children) => Ok(JellyfinCollectionDetail {
            name: set.display_name().to_string(),
            primary_image_tag: set.primary_image_tag().map(str::to_string),
            backdrop_image_tag: set.backdrop_image_tags.first().cloned(),
            items: children.items.iter().map(ItemSummary::from_dto).collect(),
            total_record_count: children.total_record_count,
            id: set.id,
        }),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn configuration_failure(error: &std::io::Error) -> ApiResponse {
    tracing::warn!(target: "collections", "collection configuration failed: {error}");
    let status = match error.kind() {
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => 400,
        std::io::ErrorKind::Unsupported => 409,
        _ => 500,
    };
    ApiResponse::error(status, error.to_string())
}

fn upload_artwork(
    services: &Arc<Services>,
    body: Body<'_, Bytes>,
) -> Result<UploadedArtwork, ApiResponse> {
    let scope = active_scope(services)?;
    match services.artwork.stage(body.bytes()) {
        Ok(id) => {
            match services
                .session
                .commit_if_current(&scope, stale_account_response, |_live| {
                    Ok(UploadedArtwork { id: id.clone() })
                }) {
                Ok(uploaded) => Ok(uploaded),
                Err(response) => {
                    if let Err(error) = services.artwork.remove(&id) {
                        tracing::warn!(target: "collections", "could not remove stale custom artwork: {error}");
                    }
                    Err(response)
                }
            }
        }
        Err(error) => Err(configuration_failure(&error)),
    }
}

fn custom_artwork(services: &Arc<Services>, id: &str) -> Result<BinaryAnswer, ApiResponse> {
    let account = active_account(services)?;
    if !services
        .collections
        .artwork_ids_for_account(&account)
        .iter()
        .any(|candidate| candidate == id)
    {
        return Err(ApiResponse::error(404, "that custom poster does not exist"));
    }
    let Some(path) = services.artwork.path(id) else {
        return Err(ApiResponse::error(404, "that custom poster does not exist"));
    };
    let content_type = match path.extension().and_then(|value| value.to_str()) {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return Err(ApiResponse::error(404, "that custom poster does not exist")),
    };
    match std::fs::read(path) {
        Ok(bytes) => Ok(BinaryAnswer::bytes(
            content_type.to_string(),
            bytes,
            IMMUTABLE_CACHE,
        )),
        Err(_) => Err(ApiResponse::error(404, "that custom poster does not exist")),
    }
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct PublicListSearchBody {
    #[serde(default)]
    query: String,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct PublicListValidateBody {
    #[serde(default)]
    selector: String,
}

#[derive(Deserialize, ts_rs::TS)]
#[ts(rename = "LocalAccountDeletionBody")]
pub(super) struct ConfirmationBody {
    #[serde(default)]
    confirmed: bool,
}

fn search_public_lists(
    services: &Arc<Services>,
    body: Body<'_, PublicListSearchBody>,
) -> Result<Value, ApiResponse> {
    let body = body.decode()?;
    active_account(services)?;
    let query = body.query.trim();
    if query.is_empty() {
        return Err(ApiResponse::error(400, "enter a public-list search"));
    }
    services
        .companion
        .search_public_lists(query)
        .map_err(|error| ApiResponse::from_api_error(&error))
}

fn validate_public_list(
    services: &Arc<Services>,
    body: Body<'_, PublicListValidateBody>,
) -> Result<Value, ApiResponse> {
    let body = body.decode()?;
    active_account(services)?;
    let selector = body.selector.trim();
    if selector.is_empty() {
        return Err(ApiResponse::error(
            400,
            "enter a public-list ID or canonical URL",
        ));
    }
    services
        .companion
        .validate_public_list(selector)
        .map_err(|error| ApiResponse::from_api_error(&error))
}

fn provider_artwork(
    services: &Arc<Services>,
    request: &ApiRequest,
) -> Result<BinaryAnswer, ApiResponse> {
    active_account(services)?;
    let Some(size) = request.param("size") else {
        return Err(ApiResponse::error(400, "artwork size is required"));
    };
    let Some(path) = request.param("path") else {
        return Err(ApiResponse::error(400, "artwork path is required"));
    };
    match services.companion.collection_artwork(&size, &path) {
        Ok((bytes, content_type)) if content_type.starts_with("image/") => {
            Ok(BinaryAnswer::bytes(content_type, bytes, IMMUTABLE_CACHE))
        }
        Ok(_) => Err(ApiResponse::error(502, "provider returned invalid artwork")),
        Err(error) => Err(ApiResponse::from_api_error(&error)),
    }
}

fn delete_local_account(
    services: &Arc<Services>,
    body: Body<'_, ConfirmationBody>,
) -> Result<AppStatus, ApiResponse> {
    let confirmed = body.decode()?.confirmed;
    if !confirmed {
        return Err(ApiResponse::error(
            400,
            "confirm the local account deletion first",
        ));
    }
    match crate::app::services::delete_local_account_data(services) {
        Ok(()) => Ok(app_status(services)),
        Err(error) => Err(ApiResponse::error(500, error)),
    }
}
