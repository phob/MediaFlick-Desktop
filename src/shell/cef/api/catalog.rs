use std::collections::BTreeMap;

use crate::library::libraries::FilterKey;
use crate::library::model::DetailView;
use crate::library::{DEFAULT_PAGE_SIZE, ItemPage};

use super::*;

mod detail;
mod home;
mod home_settings;
mod person;

pub(super) use detail::{ItemAbout, ItemSynopsis};
use detail::{children, item_about, item_detail, item_synopsis};
use home::home_resume;
pub(super) use home::{Billboard, HomeResponse, HomeResumeResponse, billboard_view, home_view};
pub(super) use home_settings::HomeSettingsResponse;
use home_settings::{home_settings, patch_home_settings};
pub(super) use person::PersonResolution;
use person::{query_person_items, resolve_person};

#[derive(Serialize, ts_rs::TS)]
pub(super) struct Genres {
    genres: Vec<String>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct OfficialRatings {
    official_ratings: Vec<String>,
}

/// Where each initial starts in the grid, `#` for any that is not A to Z.
#[derive(Serialize, ts_rs::TS)]
pub(super) struct ItemLetters {
    letters: BTreeMap<String, i64>,
}

impl ItemLetters {
    pub(super) fn new(letters: Vec<(String, i64)>) -> Self {
        Self {
            letters: letters.into_iter().collect(),
        }
    }
}

routes! {
    |services, request|
    GET ["calendar"] -> Relay = calendar(services, request),
    GET ["settings", "home"] -> HomeSettingsResponse = home_settings(services),
    PATCH ["settings", "home"] (body: HomeSettings) -> HomeSettingsResponse =
        patch_home_settings(services, body),
    GET ["home", "resume"] -> HomeResumeResponse = home_resume(services),
    GET ["home"] -> HomeResponse = home_view(services),
    GET ["billboard"] -> Billboard = billboard_view(services),
    GET ["items"] -> ItemPage = query_items(services, request),
    GET ["items", "letters"] -> ItemLetters = item_letters(services, request),
    GET ["genres"] -> Genres = genres(services),
    GET ["official-ratings"] -> OfficialRatings = official_ratings(services, request),
    GET ["person", "resolve"] -> PersonResolution = resolve_person(services, request),
    GET ["item", id] -> DetailView = item_detail(services, id),
    GET ["item", id, "synopsis"] -> ItemSynopsis = item_synopsis(services, id),
    GET ["item", id, "about"] -> ItemAbout = item_about(services, id),
    GET ["item", id, "children"] -> ItemPage = children(services, id, request),
}

fn genres(services: &Arc<Services>) -> Result<Genres, ApiResponse> {
    match services.library.genres(None) {
        Ok(genres) => Ok(Genres { genres }),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn calendar(services: &Arc<Services>, request: &ApiRequest) -> Result<Value, ApiResponse> {
    let Some(start) = request.param("start") else {
        return Err(ApiResponse::error(400, "calendar start is required"));
    };
    let Some(end) = request.param("end") else {
        return Err(ApiResponse::error(400, "calendar end is required"));
    };
    if !is_iso_date(&start) || !is_iso_date(&end) || end < start {
        return Err(ApiResponse::error(
            400,
            "calendar dates must be YYYY-MM-DD with end after start",
        ));
    }
    let scope = session_scope(services)?;
    services
        .companion
        .calendar(&start, &end)
        .map_err(|error| scoped_failure(services, &scope, &error))
}

fn is_iso_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn query_items(services: &Arc<Services>, request: &ApiRequest) -> Result<ItemPage, ApiResponse> {
    if let Some(person_id) = request.param("personId") {
        return query_person_items(services, &person_id, request);
    }

    let query = item_query(request)?;
    match services.library.query(&query) {
        Ok(page) => Ok(page),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn item_letters(
    services: &Arc<Services>,
    request: &ApiRequest,
) -> Result<ItemLetters, ApiResponse> {
    match services.library.letter_index(&item_query(request)?) {
        Ok(letters) => Ok(ItemLetters::new(letters)),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn official_ratings(
    services: &Arc<Services>,
    request: &ApiRequest,
) -> Result<OfficialRatings, ApiResponse> {
    let kinds = request
        .param("kind")
        .map(|value| {
            value
                .split(',')
                .filter_map(|kind| CatalogKind::parse(kind.trim()))
                .filter(|kind| CatalogKind::TOP_LEVEL.contains(kind))
                .collect::<Vec<_>>()
        })
        .filter(|kinds| !kinds.is_empty())
        .unwrap_or_else(|| CatalogKind::TOP_LEVEL.to_vec());
    match services.library.official_ratings(&kinds, None) {
        Ok(ratings) => Ok(OfficialRatings {
            official_ratings: ratings,
        }),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn kind_filter(request: &ApiRequest) -> Result<KindFilter, ApiResponse> {
    let value = request.param("kind").unwrap_or_default();
    let kinds = value
        .split(',')
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(|kind| {
            CatalogKind::parse(kind)
                .ok_or_else(|| ApiResponse::error(400, format!("unknown item kind {kind:?}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(KindFilter::from_kinds(kinds))
}

fn item_query(request: &ApiRequest) -> Result<ItemQuery, ApiResponse> {
    Ok(ItemQuery {
        search: request.param("search"),
        kinds: kind_filter(request)?,
        ..library_query(request, Params::Lenient)?
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Params {
    Lenient,
    Strict,
}

pub(super) fn library_query(
    request: &ApiRequest,
    params: Params,
) -> Result<ItemQuery, ApiResponse> {
    let seed = param(request, params, "seed", |value| value.parse::<u32>().ok())?;
    Ok(ItemQuery {
        sort: param(request, params, "sort", |value| {
            ItemSort::from_id(value, seed)
        })?
        .unwrap_or_default(),
        direction: param(request, params, "order", SortDirection::from_id)?,
        genre: request.param(FilterKey::Genre.id()),
        release_decade: param(
            request,
            params,
            FilterKey::Decade.id(),
            crate::library::release_decade_from_id,
        )?,
        watched: param(
            request,
            params,
            FilterKey::Watched.id(),
            WatchFilter::from_id,
        )?,
        favorite: param(
            request,
            params,
            FilterKey::Favorite.id(),
            |value| match value {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            },
        )?,
        official_rating: request
            .param(FilterKey::Parental.id())
            .map(|value| value.trim().to_string()),
        series_status: param(
            request,
            params,
            FilterKey::SeriesStatus.id(),
            SeriesStatus::from_id,
        )?,
        offset: param(request, params, "offset", |value| value.parse::<i64>().ok())?.unwrap_or(0),
        limit: param(request, params, "limit", |value| value.parse::<i64>().ok())?
            .unwrap_or(DEFAULT_PAGE_SIZE),
        ..ItemQuery::default()
    })
}

pub(super) fn param<T>(
    request: &ApiRequest,
    params: Params,
    key: &str,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Result<Option<T>, ApiResponse> {
    let Some(value) = request.param(key) else {
        return Ok(None);
    };
    match (parse(&value), params) {
        (Some(parsed), _) => Ok(Some(parsed)),
        (None, Params::Lenient) => Ok(None),
        (None, Params::Strict) => Err(ApiResponse::error(400, format!("invalid {key} {value:?}"))),
    }
}

pub(super) fn still_signed_in(
    services: &Services,
    scope: &SessionScope,
) -> Result<(), ApiResponse> {
    if services.session.scope_is_current(scope) {
        Ok(())
    } else {
        Err(stale_account_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_dates_are_strict_iso_days() {
        assert!(is_iso_date("2026-08-02"));
        assert!(!is_iso_date("2026-8-2"));
        assert!(!is_iso_date("2026/08/02"));
        assert!(!is_iso_date("../../etc"));
    }
}
