//! Row shapes for the metadata cache and their translation from Jellyfin DTOs.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::jellyfin::api::model::{BaseItemDto, UserItemDataDto};

/// One cached library item. Provider IDs get dedicated columns because they are
/// the join keys for every external metadata feature planned on top of the cache.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemRecord {
    pub jellyfin_id: String,
    pub kind: String,
    pub name: String,
    pub original_title: Option<String>,
    pub sort_name: Option<String>,
    pub year: Option<i64>,
    pub premiere_date: Option<String>,
    pub runtime_ticks: Option<i64>,
    pub overview: Option<String>,
    pub community_rating: Option<f64>,
    pub critic_rating: Option<f64>,
    pub official_rating: Option<String>,
    pub parent_id: Option<String>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub season_id: Option<String>,
    pub index_number: Option<i64>,
    pub parent_index_number: Option<i64>,
    pub child_count: Option<i64>,
    pub tmdb_id: Option<String>,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<String>,
    pub genres: String,
    pub tags: String,
    pub studios: String,
    pub people: String,
    pub image_tags: String,
    pub primary_image_tag: Option<String>,
    pub backdrop_image_tag: Option<String>,
    pub search_genres: String,
    pub search_people: String,
    pub date_created: Option<String>,
    pub date_last_saved: Option<String>,
}

/// Cast and crew kept for the details view; capped so a 200-person credit list
/// does not bloat every row.
const MAX_PEOPLE: usize = 30;

impl ItemRecord {
    pub fn from_dto(dto: &BaseItemDto) -> Self {
        let people = dto
            .people
            .iter()
            .take(MAX_PEOPLE)
            .map(|person| {
                json!({
                    "id": person.id,
                    "name": person.name,
                    "role": person.role,
                    "type": person.person_type,
                    "imageTag": person.primary_image_tag,
                })
            })
            .collect::<Vec<_>>();
        let studios = dto
            .studios
            .iter()
            .filter_map(|studio| studio.name.clone())
            .collect::<Vec<_>>();

        Self {
            jellyfin_id: dto.id.clone(),
            kind: dto
                .item_type
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            name: dto.display_name().to_string(),
            original_title: non_empty(dto.original_title.as_deref()),
            sort_name: non_empty(dto.sort_name.as_deref())
                .or_else(|| Some(dto.display_name().to_lowercase())),
            // Jellyfin normally supplies ProductionYear for both films and
            // series (for a series it is the first-air year). Older servers
            // and some metadata providers only supply PremiereDate, so keep a
            // small fallback here. The v5 database migration applies the same
            // fallback to rows that were cached before this code existed.
            year: dto.production_year.or_else(|| {
                dto.premiere_date
                    .as_deref()
                    .and_then(release_year_from_date)
            }),
            premiere_date: non_empty(dto.premiere_date.as_deref()),
            runtime_ticks: dto.run_time_ticks.filter(|ticks| *ticks > 0),
            overview: non_empty(dto.overview.as_deref()),
            community_rating: dto.community_rating,
            critic_rating: dto.critic_rating,
            official_rating: non_empty(dto.official_rating.as_deref()),
            parent_id: non_empty(dto.parent_id.as_deref()),
            series_id: non_empty(dto.series_id.as_deref()),
            series_name: non_empty(dto.series_name.as_deref()),
            season_id: non_empty(dto.season_id.as_deref()),
            index_number: dto.index_number,
            parent_index_number: dto.parent_index_number,
            child_count: dto.child_count,
            tmdb_id: dto.provider_id("Tmdb").map(str::to_string),
            imdb_id: dto.provider_id("Imdb").map(str::to_string),
            tvdb_id: dto.provider_id("Tvdb").map(str::to_string),
            genres: json_text(&dto.genres),
            tags: json_text(&dto.tags),
            studios: json_text(&studios),
            people: serde_json::to_string(&people).unwrap_or_else(|_| "[]".to_string()),
            image_tags: json_text(&dto.image_tags),
            primary_image_tag: dto.primary_image_tag().map(str::to_string),
            backdrop_image_tag: dto
                .backdrop_image_tags
                .first()
                .or_else(|| dto.parent_backdrop_image_tags.first())
                .cloned(),
            search_genres: dto.genres.join(", "),
            search_people: dto
                .people
                .iter()
                .filter_map(|person| person.name.clone())
                .take(MAX_PEOPLE)
                .collect::<Vec<_>>()
                .join(", "),
            date_created: non_empty(dto.date_created.as_deref()),
            date_last_saved: non_empty(dto.date_last_saved.as_deref()),
        }
    }
}

/// Stable quality dimensions persisted as a bitset in `metadata_convergence`.
pub const MISSING_IDENTITY: u32 = 1 << 0;
pub const MISSING_ARTWORK: u32 = 1 << 1;
pub const MISSING_DESCRIPTION: u32 = 1 << 2;
pub const MISSING_PROVIDER: u32 = 1 << 3;
pub const MISSING_STRUCTURE: u32 = 1 << 4;
pub const MISSING_RELATION: u32 = 1 << 5;
pub const MISSING_INDEX: u32 = 1 << 6;

/// Pure metadata assessment used by both ingestion and convergence polling.
/// The signature intentionally contains presentation values as well as quality
/// presence, so image-tag/title edits propagate while byte-identical REST
/// observations do not rewrite SQLite or invalidate React Query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataQuality {
    pub supported: bool,
    pub complete: bool,
    pub missing: u32,
    pub score: u8,
    pub signature: String,
}

pub fn metadata_quality(dto: &BaseItemDto) -> MetadataQuality {
    let kind = dto.item_type.as_deref().unwrap_or_default();
    let supported = matches!(kind, "Movie" | "Series" | "Season" | "Episode");
    let identity = supported
        && !dto.id.trim().is_empty()
        && dto
            .name
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty());
    let has_artwork = dto.image_tags.values().any(|tag| !tag.trim().is_empty())
        || dto
            .backdrop_image_tags
            .iter()
            .chain(dto.parent_backdrop_image_tags.iter())
            .any(|tag| !tag.trim().is_empty())
        || dto
            .series_primary_image_tag
            .as_deref()
            .is_some_and(|tag| !tag.trim().is_empty());
    let has_description = non_empty(dto.overview.as_deref()).is_some()
        || dto.genres.iter().any(|genre| !genre.trim().is_empty())
        || dto.tags.iter().any(|tag| !tag.trim().is_empty())
        || dto.studios.iter().any(|studio| {
            studio
                .name
                .as_deref()
                .is_some_and(|name| !name.trim().is_empty())
        })
        || dto.people.iter().any(|person| {
            person
                .name
                .as_deref()
                .is_some_and(|name| !name.trim().is_empty())
        })
        || non_empty(dto.official_rating.as_deref()).is_some()
        || dto.community_rating.is_some()
        || dto.critic_rating.is_some();
    let has_provider = dto
        .provider_ids
        .values()
        .any(|provider_id| !provider_id.trim().is_empty());
    let has_structure = match kind {
        "Movie" => {
            dto.production_year.is_some()
                || non_empty(dto.premiere_date.as_deref()).is_some()
                || dto.run_time_ticks.is_some_and(|ticks| ticks > 0)
        }
        "Series" => {
            dto.production_year.is_some()
                || non_empty(dto.premiere_date.as_deref()).is_some()
                || dto.child_count.is_some()
        }
        "Season" => dto.child_count.is_some(),
        "Episode" => dto.run_time_ticks.is_some_and(|ticks| ticks > 0),
        _ => false,
    };
    let has_relation = match kind {
        "Season" => {
            non_empty(dto.series_id.as_deref()).is_some()
                || non_empty(dto.parent_id.as_deref()).is_some()
        }
        "Episode" => {
            non_empty(dto.series_id.as_deref()).is_some()
                && (non_empty(dto.season_id.as_deref()).is_some()
                    || non_empty(dto.parent_id.as_deref()).is_some())
        }
        _ => true,
    };
    // `Some(0)` is valid for specials and must not be confused with missing.
    let has_index = match kind {
        "Season" => dto.index_number.is_some(),
        "Episode" => dto.index_number.is_some() && dto.parent_index_number.is_some(),
        _ => true,
    };

    let mut missing = 0;
    for (present, bit) in [
        (identity, MISSING_IDENTITY),
        (has_artwork, MISSING_ARTWORK),
        (has_description, MISSING_DESCRIPTION),
        (has_provider, MISSING_PROVIDER),
        (has_structure, MISSING_STRUCTURE),
        (has_relation, MISSING_RELATION),
        (has_index, MISSING_INDEX),
    ] {
        if !present {
            missing |= bit;
        }
    }
    let score = 7 - missing.count_ones() as u8;
    // Top-level items need multiple independent enrichment signals; one
    // provider id alone never completes them. Child items prioritize correct
    // hierarchy/indexing and require at least one useful presentation signal.
    let complete = match kind {
        "Movie" | "Series" => {
            identity && has_artwork && has_description && has_provider && has_structure
        }
        "Season" => {
            identity
                && has_relation
                && has_index
                && (has_artwork || has_description || has_provider || has_structure)
        }
        "Episode" => {
            identity
                && has_relation
                && has_index
                && has_artwork
                && (has_description || has_provider || has_structure)
        }
        _ => false,
    };
    let canonical = serde_json::to_vec(&json!({
        "type": dto.item_type,
        "name": dto.name,
        "originalTitle": dto.original_title,
        "sortName": dto.sort_name,
        "year": dto.production_year,
        "premiereDate": dto.premiere_date,
        "runtime": dto.run_time_ticks,
        "overview": dto.overview,
        "ratings": [dto.community_rating, dto.critic_rating],
        "officialRating": dto.official_rating,
        "parentId": dto.parent_id,
        "seriesId": dto.series_id,
        "seriesName": dto.series_name,
        "seasonId": dto.season_id,
        "index": dto.index_number,
        "parentIndex": dto.parent_index_number,
        "childCount": dto.child_count,
        "providers": dto.provider_ids,
        "genres": dto.genres,
        "tags": dto.tags,
        "studios": dto.studios.iter().map(|studio| &studio.name).collect::<Vec<_>>(),
        "people": dto.people.iter().map(|person| (&person.id, &person.name, &person.role,
                                                   &person.person_type, &person.primary_image_tag))
                            .collect::<Vec<_>>(),
        "images": dto.image_tags,
        "backdrops": dto.backdrop_image_tags,
        "parentBackdrops": dto.parent_backdrop_image_tags,
        "seriesPrimary": dto.series_primary_image_tag,
        "dateCreated": dto.date_created,
    }))
    .unwrap_or_default();
    let signature = canonical_fingerprint(&canonical);

    MetadataQuality {
        supported,
        complete,
        missing,
        score,
        signature,
    }
}

/// A compact deterministic fingerprint of the canonical presentation JSON.
/// Two independently seeded FNV-1a lanes keep the queue small even for DTOs
/// with large cast lists while making accidental collisions vanishingly rare.
fn canonical_fingerprint(bytes: &[u8]) -> String {
    let mut first = 0xcbf2_9ce4_8422_2325_u64;
    let mut second = 0x8422_2325_cbf2_9ce4_u64;
    for byte in bytes {
        first ^= u64::from(*byte);
        first = first.wrapping_mul(0x100_0000_01b3);
        second ^= u64::from(byte.rotate_left(1));
        second = second.wrapping_mul(0x100_0000_01b3);
    }
    format!("{first:016x}{second:016x}")
}

/// Watch state mirrored from the server and from our own playback reporting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserDataRecord {
    pub jellyfin_id: String,
    pub played: bool,
    pub play_count: i64,
    pub playback_position_ticks: i64,
    pub is_favorite: bool,
    pub played_percentage: Option<f64>,
    pub last_played_date: Option<String>,
}

impl UserDataRecord {
    pub fn from_dto(jellyfin_id: &str, dto: &UserItemDataDto) -> Self {
        Self {
            jellyfin_id: jellyfin_id.to_string(),
            played: dto.played,
            play_count: dto.play_count,
            playback_position_ticks: dto.playback_position_ticks.max(0),
            is_favorite: dto.is_favorite,
            played_percentage: dto.played_percentage,
            last_played_date: non_empty(dto.last_played_date.as_deref()),
        }
    }
}

/// Counts shown on the status card and by `--library-stats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub movies: i64,
    pub series: i64,
    pub seasons: i64,
    pub episodes: i64,
    pub total: i64,
}

fn json_text<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn release_year_from_date(value: &str) -> Option<i64> {
    let year = value.get(..4)?.parse::<i64>().ok()?;
    (1900..=9999).contains(&year).then_some(year)
}

#[cfg(test)]
mod tests {
    use super::{ItemRecord, MISSING_INDEX, UserDataRecord, metadata_quality};
    use crate::jellyfin::api::model::{BaseItemDto, UserItemDataDto};

    fn dto(json: &str) -> BaseItemDto {
        serde_json::from_str(json).expect("dto")
    }

    #[test]
    fn provider_ids_land_in_dedicated_columns() {
        let record = ItemRecord::from_dto(&dto(
            r#"{"Id":"a","Name":"The Matrix","Type":"Movie","ProductionYear":1999,
                "ProviderIds":{"Tmdb":"603","Imdb":"tt0133093"}}"#,
        ));
        assert_eq!(record.tmdb_id.as_deref(), Some("603"));
        assert_eq!(record.imdb_id.as_deref(), Some("tt0133093"));
        assert_eq!(record.tvdb_id, None);
        assert_eq!(record.year, Some(1999));
        assert_eq!(record.kind, "Movie");
    }

    #[test]
    fn release_year_prefers_production_year_and_falls_back_to_premiere_date() {
        let movie = ItemRecord::from_dto(&dto(
            r#"{"Id":"m","Name":"Movie","Type":"Movie","ProductionYear":1999,
                "PremiereDate":"2000-01-01T00:00:00Z"}"#,
        ));
        let series = ItemRecord::from_dto(&dto(r#"{"Id":"s","Name":"Series","Type":"Series",
                "PremiereDate":"2017-02-15T00:00:00Z"}"#));
        let invalid = ItemRecord::from_dto(&dto(
            r#"{"Id":"x","Name":"Unknown","Type":"Series","PremiereDate":"unknown"}"#,
        ));

        assert_eq!(movie.year, Some(1999));
        assert_eq!(series.year, Some(2017));
        assert_eq!(invalid.year, None);
    }

    #[test]
    fn missing_sort_names_fall_back_to_a_lowercased_title() {
        let record = ItemRecord::from_dto(&dto(r#"{"Id":"a","Name":"The Matrix"}"#));
        assert_eq!(record.sort_name.as_deref(), Some("the matrix"));
        assert_eq!(record.kind, "Unknown");
    }

    #[test]
    fn top_level_quality_requires_more_than_one_provider_id() {
        let sparse_movie = dto(r#"{"Id":"m","Name":"A scan in progress","Type":"Movie",
                "ProductionYear":1988,"RunTimeTicks":100,"DateCreated":"2024-01-01"}"#);
        assert!(!metadata_quality(&sparse_movie).complete);
        assert!(
            !metadata_quality(&dto(
                r#"{"Id":"m","Name":"Movie","Type":"Movie","ProductionYear":1988,
                "ProviderIds":{"Tmdb":"1"}}"#
            ))
            .complete
        );
        assert!(
            metadata_quality(&dto(
                r#"{"Id":"m","Name":"Movie","Type":"Movie","ProductionYear":1988,
                "ProviderIds":{"Tmdb":"1"},"Overview":"Description",
                "ImageTags":{"Primary":"poster"}}"#
            ))
            .complete
        );
    }

    #[test]
    fn quality_covers_every_synced_kind_and_accepts_zero_indexes() {
        for complete in [
            r#"{"Id":"m","Name":"M","Type":"Movie","ProductionYear":2000,
                "Overview":"O","ProviderIds":{"Tmdb":"1"},"ImageTags":{"Primary":"p"}}"#,
            r#"{"Id":"s","Name":"S","Type":"Series","ChildCount":0,
                "Genres":["Drama"],"ProviderIds":{"Tvdb":"1"},"ImageTags":{"Primary":"p"}}"#,
            r#"{"Id":"z","Name":"Specials","Type":"Season","SeriesId":"s",
                "IndexNumber":0,"ChildCount":2}"#,
            r#"{"Id":"e","Name":"Special","Type":"Episode","SeriesId":"s",
                "SeasonId":"z","IndexNumber":0,"ParentIndexNumber":0,
                "RunTimeTicks":1,"SeriesPrimaryImageTag":"p"}"#,
        ] {
            let quality = metadata_quality(&dto(complete));
            assert!(quality.supported);
            assert!(quality.complete, "missing mask {}", quality.missing);
            assert_eq!(quality.missing & MISSING_INDEX, 0);
        }
        let unsupported = metadata_quality(&dto(r#"{"Id":"a","Name":"Audio","Type":"Audio"}"#));
        assert!(!unsupported.supported);
        assert!(!unsupported.complete);
    }

    #[test]
    fn genres_and_people_are_flattened_for_search_and_kept_as_json() {
        let record = ItemRecord::from_dto(&dto(
            r#"{"Id":"a","Name":"Speed","Genres":["Action","Thriller"],
                "People":[{"Name":"Keanu Reeves","Type":"Actor","Role":"Jack"},
                          {"Name":"Sandra Bullock"}]}"#,
        ));
        assert_eq!(record.search_genres, "Action, Thriller");
        assert_eq!(record.search_people, "Keanu Reeves, Sandra Bullock");
        assert_eq!(record.genres, r#"["Action","Thriller"]"#);
        assert!(record.people.contains("\"role\":\"Jack\""));
    }

    #[test]
    fn people_lists_are_capped() {
        let people = (0..50)
            .map(|index| format!(r#"{{"Name":"Person {index}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let record = ItemRecord::from_dto(&dto(&format!(
            r#"{{"Id":"a","Name":"Crowd","People":[{people}]}}"#
        )));
        assert_eq!(record.search_people.split(", ").count(), 30);
    }

    #[test]
    fn zero_runtimes_and_blank_strings_become_null() {
        let record = ItemRecord::from_dto(&dto(
            r#"{"Id":"a","Name":"A","RunTimeTicks":0,"Overview":"   ","OfficialRating":""}"#,
        ));
        assert_eq!(record.runtime_ticks, None);
        assert_eq!(record.overview, None);
        assert_eq!(record.official_rating, None);
    }

    #[test]
    fn user_data_clamps_negative_positions() {
        let dto: UserItemDataDto =
            serde_json::from_str(r#"{"PlaybackPositionTicks":-5,"Played":true,"PlayCount":2}"#)
                .expect("user data");
        let record = UserDataRecord::from_dto("a", &dto);
        assert_eq!(record.playback_position_ticks, 0);
        assert!(record.played);
        assert_eq!(record.play_count, 2);
    }
}
