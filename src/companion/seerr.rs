use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::jellyfin::api::ApiError;
use crate::jellyfin::session::SessionScope;
use crate::library::external::external_links_for_discovery;
use crate::library::{CatalogKind, Library, ProviderIds, TitleKind};
use crate::seerr::{DiscoverKind, DiscoverOptions, RequestProfileSelection};

use super::CompanionSession;

const STUDIO_NETWORK_DISCOVERY: &str = "seerr-discovery-v5";

pub const MAX_COMPANY_QUERY_CHARS: usize = 100;

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct SeerrCompany {
    id: i64,
    name: String,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SeerrCompanyPage {
    page: i64,
    total_pages: i64,
    total_results: i64,
    results: Vec<SeerrCompany>,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct SeerrNetworkList {
    networks: Vec<SeerrCompany>,
}

impl CompanionSession {
    fn get_seerr(
        &self,
        scope: &SessionScope,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Value, ApiError> {
        self.fetch(scope, |client, _| client.companion_get_json(path, query))
    }

    fn post_seerr(
        &self,
        scope: &SessionScope,
        path: &str,
        body: &Value,
    ) -> Result<Value, ApiError> {
        self.fetch(scope, |client, _| {
            client.companion_post_json_once(path, body)
        })
    }

    pub fn seerr_status(&self, scope: &SessionScope) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr(scope, "/MediaFlick/seerr/status", &[])
    }

    pub fn seerr_search(
        &self,
        scope: &SessionScope,
        query: &str,
        page: i64,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
            scope,
            "/MediaFlick/seerr/search",
            &[("query", query.to_string()), ("page", page.to_string())],
        )?;
        join_seerr_results(&self.library, &mut value, OtherKinds::Drop)?;
        Ok(value)
    }

    pub fn seerr_person_credits(
        &self,
        scope: &SessionScope,
        tmdb_id: i64,
    ) -> Result<Value, ApiError> {
        self.require_capability(
            "seerr-person-discovery",
            "the MediaFlick Companion plugin must be updated for cast discovery",
        )?;
        let mut value = self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/person/{tmdb_id}/credits"),
            &[],
        )?;
        join_seerr_results(&self.library, &mut value, OtherKinds::Drop)?;
        Ok(value)
    }

    pub fn seerr_discover(
        &self,
        scope: &SessionScope,
        kind: DiscoverKind,
        page: i64,
        options: &DiscoverOptions,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let query = options.query_pairs(kind, page);
        if query
            .iter()
            .any(|(name, _)| matches!(*name, "studio" | "network"))
        {
            self.require_capability(
                STUDIO_NETWORK_DISCOVERY,
                "the MediaFlick Companion plugin must be updated for studio and network filters",
            )?;
        }
        let mut value = self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/discover/{}", kind.id()),
            query.as_slice(),
        )?;
        join_seerr_results(&self.library, &mut value, OtherKinds::Drop)?;
        Ok(value)
    }

    pub fn seerr_companies(
        &self,
        scope: &SessionScope,
        query: &str,
        page: i64,
    ) -> Result<SeerrCompanyPage, ApiError> {
        self.require_capability(
            STUDIO_NETWORK_DISCOVERY,
            "the MediaFlick Companion plugin must be updated for studio search",
        )?;
        self.fetch(scope, |client, _| {
            client.companion_get_json(
                "/MediaFlick/seerr/companies",
                &[("query", query.to_string()), ("page", page.to_string())],
            )
        })
    }

    pub fn seerr_networks(&self, scope: &SessionScope) -> Result<SeerrNetworkList, ApiError> {
        self.require_capability(
            STUDIO_NETWORK_DISCOVERY,
            "the MediaFlick Companion plugin must be updated for network filters",
        )?;
        self.fetch(scope, |client, _| {
            client.companion_get_json("/MediaFlick/seerr/networks", &[])
        })
    }

    pub fn seerr_genres(&self, scope: &SessionScope, media_type: &str) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/genres/{media_type}"),
            &[],
        )
    }

    pub fn seerr_media(
        &self,
        scope: &SessionScope,
        media_type: &str,
        tmdb_id: i64,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/media/{media_type}/{tmdb_id}"),
            &[],
        )?;
        join_seerr_item(&self.library, &mut value)?;
        attach_discovery_external_links(&mut value);
        Ok(value)
    }

    pub fn seerr_request_options(
        &self,
        scope: &SessionScope,
        media_type: &str,
        is_4k: bool,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.get_seerr(
            scope,
            &format!("/MediaFlick/seerr/request-options/{media_type}"),
            &[("is4k", is_4k.to_string())],
        )
    }

    pub fn seerr_create_request(
        &self,
        scope: &SessionScope,
        media_type: &str,
        tmdb_id: i64,
        seasons: Option<&[i64]>,
        is_4k: bool,
        profile: Option<RequestProfileSelection>,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.post_seerr(
            scope,
            "/MediaFlick/seerr/request",
            &json!({
                "mediaType": media_type,
                "tmdbId": tmdb_id,
                "seasons": seasons,
                "is4k": is_4k,
                "serverId": profile.map(|selection| selection.server_id),
                "profileId": profile.map(|selection| selection.profile_id),
            }),
        )?;
        join_seerr_item(&self.library, &mut value)?;
        Ok(value)
    }

    pub fn seerr_requests(
        &self,
        scope: &SessionScope,
        take: i64,
        skip: i64,
        filter: &str,
    ) -> Result<Value, ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        let mut value = self.get_seerr(
            scope,
            "/MediaFlick/seerr/requests",
            &[
                ("take", take.to_string()),
                ("skip", skip.to_string()),
                ("filter", filter.to_string()),
            ],
        )?;
        join_seerr_results(&self.library, &mut value, OtherKinds::Keep)?;
        Ok(value)
    }

    pub fn seerr_cancel_request(
        &self,
        scope: &SessionScope,
        request_id: i64,
    ) -> Result<(), ApiError> {
        self.require_capability("seerr", "MediaFlick Companion does not provide Seerr")?;
        self.fetch(scope, |client, _| {
            client.companion_delete_once(&format!("/MediaFlick/seerr/request/{request_id}"))
        })?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OtherKinds {
    Drop,
    Keep,
}

fn join_seerr_results(
    library: &Library,
    value: &mut Value,
    other_kinds: OtherKinds,
) -> Result<(), ApiError> {
    let Some(results) = value.get_mut("results").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let mut kinds = Vec::with_capacity(results.len());
    let mut movies = Vec::new();
    let mut series = Vec::new();
    results.retain(|item| {
        let kind = seerr_kind(item);
        if kind.is_none() && other_kinds == OtherKinds::Drop {
            return false;
        }
        if let Some(tmdb_id) = item["tmdbId"].as_i64() {
            match kind {
                Some(TitleKind::Movie) => movies.push(tmdb_id.to_string()),
                Some(TitleKind::Series) => series.push(tmdb_id.to_string()),
                None => {}
            }
        }
        kinds.push(kind);
        true
    });
    let movie_ids = library.ids_by_tmdb(CatalogKind::Movie, &movies)?;
    let series_ids = library.ids_by_tmdb(CatalogKind::Series, &series)?;
    let movie_played = library.played_by_tmdb(CatalogKind::Movie, &movies)?;
    for (item, kind) in results.iter_mut().zip(kinds) {
        let Some(tmdb_id) = item["tmdbId"].as_i64().map(|id| id.to_string()) else {
            continue;
        };
        let (owned, played) = match kind {
            Some(TitleKind::Movie) => (movie_ids.get(&tmdb_id), movie_played.get(&tmdb_id)),
            Some(TitleKind::Series) => (series_ids.get(&tmdb_id), None),
            None => (None, None),
        };
        item["libraryItemId"] = owned.map_or(Value::Null, |id| json!(id));
        item["played"] = json!(played.copied().unwrap_or(false));
    }
    Ok(())
}

fn seerr_kind(item: &Value) -> Option<TitleKind> {
    match item["mediaType"].as_str()? {
        "movie" => Some(TitleKind::Movie),
        "tv" => Some(TitleKind::Series),
        _ => None,
    }
}

fn join_seerr_item(library: &Library, value: &mut Value) -> Result<(), ApiError> {
    let Some(tmdb_id) = value["tmdbId"].as_i64() else {
        return Ok(());
    };
    let library_item_id = match seerr_kind(value) {
        Some(kind) => library
            .ids_by_tmdb(kind.into(), &[tmdb_id.to_string()])?
            .remove(&tmdb_id.to_string()),
        None => None,
    };
    value["libraryItemId"] = library_item_id.map_or(Value::Null, Value::String);
    Ok(())
}

fn attach_discovery_external_links(value: &mut Value) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    let kind = match map.get("mediaType").and_then(Value::as_str) {
        Some("movie") => "Movie",
        Some("tv") => "Series",
        _ => {
            map.insert("externalLinks".to_string(), json!([]));
            return;
        }
    };
    let title = map.get("title").and_then(Value::as_str).unwrap_or_default();
    let year = map.get("year").and_then(Value::as_i64);
    let tmdb = map
        .get("tmdbId")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
        .map(|id| id.to_string());
    let external_ids = map.get("externalIds");
    let imdb = external_ids
        .and_then(|ids| ids.get("imdb"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let tvdb = external_ids.and_then(|ids| ids.get("tvdb")).and_then(|id| {
        id.as_i64()
            .filter(|number| *number > 0)
            .map(|number| number.to_string())
            .or_else(|| id.as_str().map(str::to_string))
    });
    let ids = ProviderIds { tmdb, imdb, tvdb };
    map.insert(
        "externalLinks".to_string(),
        json!(external_links_for_discovery(kind, title, year, &ids)),
    );
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{OtherKinds, attach_discovery_external_links, join_seerr_results};
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::jellyfin::session::Live;
    use crate::library::{Library, UserDataRecord};

    #[test]
    fn seerr_rows_carry_ownership_and_watched_state() {
        let library = Library::open_in_memory().expect("library");
        let items = [
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"m1","Name":"The Matrix","Type":"Movie","ProviderIds":{"Tmdb":"603"}}"#,
            )
            .expect("movie"),
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"m2","Name":"Alien","Type":"Movie","ProviderIds":{"Tmdb":"348"}}"#,
            )
            .expect("movie"),
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"s1","Name":"The 603","Type":"Series","ProviderIds":{"Tmdb":"603"}}"#,
            )
            .expect("series"),
        ];
        library
            .upsert_page(&Live::for_test(), &items)
            .expect("seed");
        library
            .apply_user_data(
                &Live::for_test(),
                &[UserDataRecord {
                    jellyfin_id: "m1".to_string(),
                    played: true,
                    ..Default::default()
                }],
            )
            .expect("user data");
        let mut page = json!({
            "results": [
                { "mediaType": "movie", "tmdbId": 603 },
                { "mediaType": "movie", "tmdbId": 348 },
                { "mediaType": "movie", "tmdbId": 624834 },
                { "mediaType": "tv", "tmdbId": 603 }
            ]
        });

        join_seerr_results(&library, &mut page, OtherKinds::Keep).expect("join");

        assert_eq!(page["results"][0]["libraryItemId"], "m1");
        assert_eq!(page["results"][0]["played"], true);
        assert_eq!(page["results"][1]["libraryItemId"], "m2");
        assert_eq!(page["results"][1]["played"], false);
        assert!(page["results"][2]["libraryItemId"].is_null());
        assert_eq!(page["results"][2]["played"], false);
        assert_eq!(page["results"][3]["libraryItemId"], "s1");
    }

    #[test]
    fn seerr_media_attaches_canonical_external_links_and_handles_missing_external_ids() {
        let mut series = json!({
            "mediaType": "tv",
            "tmdbId": 95396,
            "title": "Severance",
            "year": 2022,
            "externalIds": { "imdb": "tt11280740", "tvdb": 371980 }
        });
        attach_discovery_external_links(&mut series);
        assert_eq!(
            series["externalLinks"],
            json!([
                { "kind": "url", "id": "imdb", "label": "IMDb", "href": "https://www.imdb.com/title/tt11280740/" },
                { "kind": "url", "id": "tmdb", "label": "TMDB", "href": "https://www.themoviedb.org/tv/95396" },
                { "kind": "url", "id": "tvdb", "label": "TVDB", "href": "https://thetvdb.com/dereferrer/series/371980" },
                { "kind": "url", "id": "trakt", "label": "Trakt", "href": "https://trakt.tv/shows/tt11280740" },
                {
                    "kind": "url",
                    "id": "rotten-tomatoes-search",
                    "label": "Rotten Tomatoes",
                    "actionLabel": "Search Rotten Tomatoes",
                    "href": "https://www.rottentomatoes.com/search?search=Severance%202022"
                }
            ])
        );

        let mut legacy = json!({
            "mediaType": "tv",
            "tmdbId": 95396,
            "title": "Severance",
            "year": null
        });
        attach_discovery_external_links(&mut legacy);
        assert_eq!(
            legacy["externalLinks"],
            json!([
                { "kind": "url", "id": "tmdb", "label": "TMDB", "href": "https://www.themoviedb.org/tv/95396" },
                {
                    "kind": "url",
                    "id": "rotten-tomatoes-search",
                    "label": "Rotten Tomatoes",
                    "actionLabel": "Search Rotten Tomatoes",
                    "href": "https://www.rottentomatoes.com/search?search=Severance"
                }
            ])
        );
    }

    #[test]
    fn seerr_media_external_links_ignore_non_object_payloads_without_panicking() {
        for original in [
            json!([1, 2]),
            json!("text"),
            json!(42),
            serde_json::Value::Null,
        ] {
            let mut payload = original.clone();
            attach_discovery_external_links(&mut payload);
            assert_eq!(payload, original);
        }
    }
}
