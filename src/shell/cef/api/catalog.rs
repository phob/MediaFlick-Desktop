use crate::library::DEFAULT_PAGE_SIZE;
use crate::library::libraries::FilterKey;

use super::*;

mod detail;
mod home;
mod home_settings;
mod person;

use detail::{children, item_about, item_detail, item_synopsis};
use home::{billboard, home, home_resume};
use home_settings::{home_settings, patch_home_settings};
use person::{query_person_items, resolve_person};

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
    let response = match segments {
        ["calendar"] if request.is("GET") => calendar(services, request),
        ["settings", "home"] if request.is("GET") => home_settings(services),
        ["settings", "home"] if request.is("PATCH") => patch_home_settings(services, request),
        ["home", "resume"] if request.is("GET") => home_resume(services),
        ["home"] if request.is("GET") => home(services),
        ["billboard"] if request.is("GET") => billboard(services),
        ["items"] if request.is("GET") => query_items(services, request),
        ["items", "letters"] if request.is("GET") => item_letters(services, request),
        ["genres"] if request.is("GET") => Ok(match services.library.genres(None) {
            Ok(genres) => ApiResponse::untyped(json!({ "genres": genres })),
            Err(error) => storage_failure(&error),
        }),
        ["official-ratings"] if request.is("GET") => official_ratings(services, request),
        ["person", "resolve"] if request.is("GET") => resolve_person(services, request),
        ["item", id] if request.is("GET") => item_detail(services, id),
        ["item", id, "synopsis"] if request.is("GET") => item_synopsis(services, id),
        ["item", id, "about"] if request.is("GET") => item_about(services, id),
        ["item", id, "children"] if request.is("GET") => children(services, id, request),
        _ => return None,
    };
    Some(response)
}

fn calendar(services: &Arc<Services>, request: &ApiRequest) -> Handled {
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
    match services.companion.calendar(&start, &end) {
        Ok(value) => Ok(ApiResponse::untyped(value)),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
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

fn query_items(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    if let Some(person_id) = request.param("personId") {
        return query_person_items(services, &person_id, request);
    }

    let query = item_query(request)?;
    match services.library.query(&query) {
        Ok(page) => Ok(ApiResponse::untyped(
            json!({ "items": page.items, "total": page.total }),
        )),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn item_letters(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    match services.library.letter_index(&item_query(request)?) {
        Ok(letters) => Ok(ApiResponse::untyped(json!({
            "letters": letters.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        }))),
        Err(error) => Err(storage_failure(&error)),
    }
}

fn official_ratings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
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
        Ok(ratings) => Ok(ApiResponse::untyped(json!({ "officialRatings": ratings }))),
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
