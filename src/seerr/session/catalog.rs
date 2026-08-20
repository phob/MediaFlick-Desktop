use serde_json::{Value, json};

use super::SeerrSession;
use crate::seerr::api::error::SeerrError;
use crate::seerr::api::model::{
    self as model, MediaDetail, MediaInfo, PersonCombinedCredits, SearchPage, SearchResult,
};
use crate::seerr::discovery::{DiscoverKind, DiscoverOptions};

impl SeerrSession {
    /// Titles matching `query`, joined against the local cache.
    ///
    /// `person` results are dropped: they carry no TMDB media id, have no
    /// Jellyfin counterpart, and nothing about them can be requested.
    pub fn search(&self, query: &str, page: i64) -> Result<Value, SeerrError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(empty_page());
        }
        let page = page.clamp(1, 1_000);
        let results: SearchPage = self.call(|client| {
            client.get_json(
                "search",
                &[
                    ("query", query.to_string()),
                    ("page", page.to_string()),
                    ("language", "en".to_string()),
                ],
            )
        })?;
        Ok(self.joined_page(results))
    }

    /// Discoverable movie and series cast credits for one exact TMDB person.
    /// Multiple character credits can repeat a title, so collapse them by the
    /// stable media namespace and TMDB id before joining request/local state.
    pub fn person_credits(&self, tmdb_id: i64) -> Result<Value, SeerrError> {
        if tmdb_id <= 0 {
            return Err(SeerrError::Unusable(
                "that is not a TMDB person id".to_string(),
            ));
        }
        let path = format!("person/{tmdb_id}/combined_credits");
        let credits: PersonCombinedCredits =
            self.call(|client| client.get_json(&path, &[("language", "en".to_string())]))?;
        if credits.id != 0 && credits.id != tmdb_id {
            return Err(SeerrError::Decode(
                "person credits addressed a different TMDB person".to_string(),
            ));
        }

        let mut positions: std::collections::HashMap<(String, i64), usize> =
            std::collections::HashMap::new();
        let mut results: Vec<SearchResult> = Vec::new();
        for credit in credits.cast.into_iter().filter(|credit| {
            credit.is_media()
                && credit.id > 0
                && !credit.adult
                && !credit
                    .character
                    .as_deref()
                    .is_some_and(|role| role.trim().eq_ignore_ascii_case("Thanks"))
        }) {
            let key = (credit.media_type.clone(), credit.id);
            if let Some(index) = positions.get(&key).copied() {
                // Duplicate character credits sometimes differ only in whether
                // Seerr attached `mediaInfo`; retain that request state.
                if results[index].media_info.is_none() && credit.media_info.is_some() {
                    results[index].media_info = credit.media_info;
                }
                continue;
            }
            positions.insert(key, results.len());
            results.push(credit);
        }

        let total = i64::try_from(results.len()).unwrap_or(i64::MAX);
        Ok(self.joined_page(SearchPage {
            page: 1,
            total_pages: i64::from(total > 0),
            total_results: total,
            results,
        }))
    }

    /// One of Seerr's discovery rows. The kind is validated by the caller, so a
    /// path segment from the UI never reaches the address unchecked.
    pub fn discover(
        &self,
        kind: DiscoverKind,
        page: i64,
        options: &DiscoverOptions,
    ) -> Result<Value, SeerrError> {
        let query = options.query_pairs(kind, page);
        let results: SearchPage =
            self.call(|client| client.get_json(kind.path(), query.as_slice()))?;
        Ok(self.joined_page(results))
    }

    /// The genre cards Seerr builds from TMDB, including their backdrop art.
    pub fn genres(&self, media_type: &str) -> Result<Value, SeerrError> {
        let path = match media_type {
            model::MOVIE => "discover/genreslider/movie",
            model::TV => "discover/genreslider/tv",
            _ => {
                return Err(SeerrError::Unusable(
                    "genres are only available for movies and series".to_string(),
                ));
            }
        };
        self.call(|client| client.get_json(path, &[]))
    }

    /// One title in full, with the local item it corresponds to if the library
    /// already has it.
    pub fn media_detail(&self, media_type: &str, tmdb_id: i64) -> Result<Value, SeerrError> {
        let Some(kind) = model::library_kind(media_type) else {
            return Err(SeerrError::Unusable(format!(
                "{media_type} is not a kind of title that can be requested"
            )));
        };
        if tmdb_id <= 0 {
            return Err(SeerrError::Unusable("that is not a TMDB id".to_string()));
        }
        let path = format!("{media_type}/{tmdb_id}");
        let detail: MediaDetail = self.call(|client| client.get_json(&path, &[]))?;

        let library_item_id = self
            .library
            .ids_by_tmdb(kind, &[tmdb_id.to_string()])
            .unwrap_or_default()
            .remove(&tmdb_id.to_string());
        let info = detail.media_info.clone().unwrap_or_default();
        let title = detail.display_title().to_string();
        let year = detail.year();
        let runtime = detail.runtime_minutes();
        let original_title = detail
            .original_title
            .clone()
            .or_else(|| detail.original_name.clone());
        let release_date = detail
            .release_date
            .clone()
            .or_else(|| detail.first_air_date.clone());
        let DetailPeople {
            studios,
            networks,
            creators,
            directors,
            writers,
            production_countries,
            spoken_languages,
            cast,
        } = detail_people(&detail);
        let genres = names_of(&detail.genres);
        let seasons = season_list(&detail, &info);
        let trailer = trailer_of(&detail);
        let release_dates = release_dates_of(&detail);
        let content_ratings = content_ratings_of(&detail);
        let external_ids = detail_external_ids(&detail);
        let next_episode = detail.next_episode_to_air.as_ref().map(|episode| {
            json!({
                "name": episode.name,
                "airDate": episode.air_date,
                "seasonNumber": episode.season_number,
                "episodeNumber": episode.episode_number,
            })
        });
        let mut response = json!({
            "mediaType": media_type,
            "tmdbId": detail.id,
            "title": title,
            "year": year,
            "overview": detail.overview,
            "tagline": detail.tagline,
            "originalTitle": original_title,
            "posterPath": detail.poster_path,
            "backdropPath": detail.backdrop_path,
            "voteAverage": detail.vote_average,
            "voteCount": detail.vote_count,
            "runtimeMinutes": runtime,
            "genres": genres,
            "status": model::status_name(info.status),
            "status4k": model::status_name(info.status_4k),
            "libraryItemId": library_item_id,
            "seasons": seasons,
            "releaseDate": release_date,
            "firstAirDate": detail.first_air_date,
            "lastAirDate": detail.last_air_date,
            "productionStatus": detail.status,
            "inProduction": detail.in_production,
            "seriesType": detail.series_type,
            "numberOfSeasons": detail.number_of_seasons,
            "numberOfEpisodes": detail.number_of_episodes,
            "originalLanguage": detail.original_language,
            "homepage": detail.homepage,
            "budget": positive(detail.budget),
            "revenue": positive(detail.revenue),
            "studios": studios,
            "networks": networks,
            "creators": creators,
            "directors": directors,
            "writers": writers,
            "productionCountries": production_countries,
            "spokenLanguages": spoken_languages,
            "cast": cast,
            "trailer": trailer,
            "releaseDates": release_dates,
            "contentRatings": content_ratings,
            "nextEpisode": next_episode,
        });
        if let Value::Object(fields) = &mut response {
            fields.insert("externalIds".to_string(), external_ids);
        }
        Ok(response)
    }

    /// Turns one page of Seerr results into the shape the UI renders, with the
    /// local cache joined in.
    ///
    /// The join is one query per media type rather than one per result: a page
    /// of twenty titles costs two lookups, not twenty.
    fn joined_page(&self, page: SearchPage) -> Value {
        let media = page
            .results
            .into_iter()
            .filter(SearchResult::is_media)
            .collect::<Vec<_>>();
        let ids_of = |media_type: &str| {
            media
                .iter()
                .filter(|result| result.media_type == media_type)
                .map(|result| result.id.to_string())
                .collect::<Vec<_>>()
        };
        let movies = self.local_ids(model::MOVIE, &ids_of(model::MOVIE));
        let series = self.local_ids(model::TV, &ids_of(model::TV));

        let results = media
            .iter()
            .map(|result| {
                let owned = if result.media_type == model::MOVIE {
                    &movies
                } else {
                    &series
                };
                let info = result.media_info.clone().unwrap_or_default();
                json!({
                    "mediaType": result.media_type,
                    "tmdbId": result.id,
                    "title": result.display_title(),
                    "year": result.year(),
                    "overview": result.overview,
                    "posterPath": result.poster_path,
                    "backdropPath": result.backdrop_path,
                    "voteAverage": result.vote_average,
                    "status": model::status_name(info.status),
                    "status4k": model::status_name(info.status_4k),
                    // The killer join: every result resolves to "play it" or
                    // "request it", never to a dead end.
                    "libraryItemId": owned.get(&result.id.to_string()),
                })
            })
            .collect::<Vec<_>>();

        json!({
            "page": page.page,
            "totalPages": page.total_pages,
            "totalResults": page.total_results,
            "results": results,
        })
    }

    /// TMDB id → Jellyfin id for one media type. A storage failure degrades to
    /// "the library does not have it", which offers a request rather than
    /// failing a whole page of results.
    pub(super) fn local_ids(
        &self,
        media_type: &str,
        tmdb_ids: &[String],
    ) -> std::collections::HashMap<String, String> {
        let Some(kind) = model::library_kind(media_type) else {
            return Default::default();
        };
        self.library
            .ids_by_tmdb(kind, tmdb_ids)
            .unwrap_or_else(|error| {
                tracing::warn!(
                    target: "seerr.session",
                    "could not join Seerr results against the library: {error}"
                );
                Default::default()
            })
    }
}

/// The answer for a search with nothing to search for, so an empty field costs
/// no round trip.
fn empty_page() -> Value {
    json!({ "page": 1, "totalPages": 0, "totalResults": 0, "results": [] })
}

fn positive(value: Option<i64>) -> Option<i64> {
    value.filter(|value| *value > 0)
}

struct DetailPeople {
    studios: Vec<String>,
    networks: Vec<String>,
    creators: Vec<String>,
    directors: Vec<String>,
    writers: Vec<String>,
    production_countries: Vec<Value>,
    spoken_languages: Vec<Value>,
    cast: Vec<Value>,
}

fn detail_external_ids(detail: &MediaDetail) -> Value {
    json!({
        "imdb": detail.external_imdb_id(),
        "tvdb": detail.external_tvdb_id(),
    })
}

fn detail_people(detail: &MediaDetail) -> DetailPeople {
    let creators = unique_names(
        detail
            .created_by
            .iter()
            .map(|person| person.name.as_str())
            .chain(crew_names(detail, |credit| {
                credit.job.as_deref() == Some("Creator")
            })),
    );
    let directors = unique_names(crew_names(detail, |credit| {
        credit.job.as_deref() == Some("Director")
    }));
    let writers = unique_names(crew_names(detail, |credit| {
        credit.department.as_deref() == Some("Writing")
            || matches!(
                credit.job.as_deref(),
                Some("Writer" | "Screenplay" | "Story" | "Teleplay")
            )
    }));
    let production_countries = detail
        .production_countries
        .iter()
        .map(|country| json!({ "code": country.code, "name": country.name }))
        .collect();
    let spoken_languages = detail
        .spoken_languages
        .iter()
        .map(|language| {
            json!({
                "code": language.code,
                "name": language.english_name.as_deref().unwrap_or(&language.name),
            })
        })
        .collect();
    let cast = detail
        .credits
        .cast
        .iter()
        .take(20)
        .map(|person| {
            json!({
                "id": person.id,
                "name": person.name,
                "character": person.character,
                "profilePath": person.profile_path,
            })
        })
        .collect();
    DetailPeople {
        studios: names_of(&detail.production_companies),
        networks: names_of(&detail.networks),
        creators,
        directors,
        writers,
        production_countries,
        spoken_languages,
        cast,
    }
}

fn names_of(values: &[model::NamedId]) -> Vec<String> {
    unique_names(values.iter().map(|value| value.name.as_str()))
}

fn crew_names<'a>(
    detail: &'a MediaDetail,
    predicate: impl Fn(&model::Credit) -> bool + 'a,
) -> impl Iterator<Item = &'a str> + 'a {
    detail
        .credits
        .crew
        .iter()
        .filter(move |credit| predicate(credit))
        .map(|credit| credit.name.as_str())
}

fn unique_names<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut names = Vec::new();
    for value in values.map(str::trim).filter(|value| !value.is_empty()) {
        if !names
            .iter()
            .any(|known: &String| known.eq_ignore_ascii_case(value))
        {
            names.push(value.to_string());
        }
    }
    names
}

fn trailer_of(detail: &MediaDetail) -> Option<Value> {
    detail
        .related_videos
        .iter()
        .filter(|video| {
            video.site == "YouTube"
                && video.video_type == "Trailer"
                && video.key.len() == 11
                && video
                    .key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .max_by_key(|video| video.size)
        .map(|video| {
            json!({
                "name": if video.name.trim().is_empty() {
                    "Trailer"
                } else {
                    video.name.as_str()
                },
                "key": video.key,
            })
        })
}

fn release_dates_of(detail: &MediaDetail) -> Vec<Value> {
    detail
        .releases
        .results
        .iter()
        .flat_map(|country| {
            country.release_dates.iter().filter_map(|release| {
                let kind = match release.release_type {
                    1 => "premiere",
                    2 => "limited-cinema",
                    3 => "cinema",
                    4 => "digital",
                    5 => "physical",
                    6 => "tv",
                    _ => return None,
                };
                (!country.region.trim().is_empty() && !release.release_date.trim().is_empty()).then(
                    || {
                        json!({
                            "region": country.region,
                            "type": kind,
                            "date": release.release_date,
                            "certification": release.certification,
                        })
                    },
                )
            })
        })
        .collect()
}

fn content_ratings_of(detail: &MediaDetail) -> Vec<Value> {
    detail
        .content_ratings
        .results
        .iter()
        .filter(|rating| !rating.region.trim().is_empty() && !rating.rating.trim().is_empty())
        .map(|rating| {
            json!({
                "region": rating.region,
                "rating": rating.rating,
            })
        })
        .collect()
}

/// The seasons a series can be requested by, each carrying what Seerr already
/// has of it.
///
/// Season 0 is left out, as Seerr's own request modal leaves it out: specials
/// are not a season the *arr side tracks as requestable.
fn season_list(detail: &MediaDetail, info: &MediaInfo) -> Vec<Value> {
    detail
        .seasons
        .iter()
        .filter(|season| season.season_number >= 1)
        .map(|season| {
            let known = info
                .seasons
                .iter()
                .find(|entry| entry.season_number == season.season_number);
            let status = known
                .map(|entry| entry.status)
                .unwrap_or(model::media_status::UNKNOWN);
            let status_4k = known
                .map(|entry| entry.status_4k)
                .unwrap_or(model::media_status::UNKNOWN);
            json!({
                "seasonNumber": season.season_number,
                "name": season.name,
                "episodeCount": season.episode_count,
                "airDate": season.air_date,
                "status": model::status_name(status),
                "status4k": model::status_name(status_4k),
            })
        })
        .collect()
}
