use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionMode {
    MediaFlick,
    #[default]
    Jellyfin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaType {
    #[default]
    Movie,
    Series,
    Mixed,
}

impl MediaType {
    pub fn identity_name(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Series => "series",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResultOrdering {
    #[default]
    Source,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshCadence {
    #[default]
    Manual,
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ResultLimit {
    #[default]
    All,
    Maximum {
        count: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateReference {
    pub id: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CollectionSource {
    TmdbDiscover {
        schema_version: u32,
        parameters: BTreeMap<String, Value>,
    },
    TmdbCollection {
        schema_version: u32,
        collection_id: u64,
        include_unreleased: bool,
    },
    MdbListPublicList {
        schema_version: u32,
        list_id: String,
    },
    /// A source shape written by a newer app or a removed implementation.
    /// Keep its exact JSON so unrelated profiles remain usable and future
    /// builds can recover it without a lossy rewrite.
    Unsupported { raw: Value },
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum KnownCollectionSource {
    TmdbDiscover {
        #[serde(default = "source_schema_version")]
        schema_version: u32,
        #[serde(default)]
        parameters: BTreeMap<String, Value>,
    },
    TmdbCollection {
        #[serde(default = "source_schema_version")]
        schema_version: u32,
        collection_id: u64,
        #[serde(default)]
        include_unreleased: bool,
    },
    MdbListPublicList {
        #[serde(default = "source_schema_version")]
        schema_version: u32,
        list_id: String,
    },
}

impl Serialize for CollectionSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::TmdbDiscover {
                schema_version,
                parameters,
            } => KnownCollectionSource::TmdbDiscover {
                schema_version: *schema_version,
                parameters: parameters.clone(),
            }
            .serialize(serializer),
            Self::TmdbCollection {
                schema_version,
                collection_id,
                include_unreleased,
            } => KnownCollectionSource::TmdbCollection {
                schema_version: *schema_version,
                collection_id: *collection_id,
                include_unreleased: *include_unreleased,
            }
            .serialize(serializer),
            Self::MdbListPublicList {
                schema_version,
                list_id,
            } => KnownCollectionSource::MdbListPublicList {
                schema_version: *schema_version,
                list_id: list_id.clone(),
            }
            .serialize(serializer),
            Self::Unsupported { raw } => raw.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CollectionSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = Value::deserialize(deserializer)?;
        let source = match serde_json::from_value::<KnownCollectionSource>(raw.clone()) {
            Ok(KnownCollectionSource::TmdbDiscover {
                schema_version,
                parameters,
            }) => Self::TmdbDiscover {
                schema_version,
                parameters,
            },
            Ok(KnownCollectionSource::TmdbCollection {
                schema_version,
                collection_id,
                include_unreleased,
            }) => Self::TmdbCollection {
                schema_version,
                collection_id,
                include_unreleased,
            },
            Ok(KnownCollectionSource::MdbListPublicList {
                schema_version,
                list_id,
            }) => Self::MdbListPublicList {
                schema_version,
                list_id,
            },
            Err(_) => Self::Unsupported { raw },
        };
        Ok(source)
    }
}

impl CollectionSource {
    pub fn provider(&self) -> Option<Provider> {
        match self {
            Self::TmdbDiscover { .. } | Self::TmdbCollection { .. } => Some(Provider::Tmdb),
            Self::MdbListPublicList { .. } => Some(Provider::MdbList),
            Self::Unsupported { .. } => None,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::TmdbDiscover { schema_version, .. }
            | Self::TmdbCollection { schema_version, .. }
            | Self::MdbListPublicList { schema_version, .. }
                if *schema_version != source_schema_version() =>
            {
                Err("this collection source version is not supported")
            }
            Self::TmdbDiscover { parameters, .. } => validate_discover_parameters(parameters),
            Self::TmdbCollection {
                collection_id: 0, ..
            } => Err("the TMDB collection id must be positive"),
            Self::MdbListPublicList { list_id, .. } if !valid_mdblist_id(list_id) => {
                Err("the MDBList public list id is invalid")
            }
            Self::Unsupported { .. } => Err("this collection source is not supported"),
            _ => Ok(()),
        }
    }

    fn supports_media_type(&self, media_type: MediaType) -> bool {
        match self {
            Self::TmdbDiscover { .. } => matches!(media_type, MediaType::Movie | MediaType::Series),
            Self::TmdbCollection { .. } => media_type == MediaType::Movie,
            Self::MdbListPublicList { .. } => true,
            Self::Unsupported { .. } => false,
        }
    }
}

fn validate_discover_parameters(parameters: &BTreeMap<String, Value>) -> Result<(), &'static str> {
    for (key, value) in parameters {
        let valid = match key.as_str() {
            "feed" => value.as_str() == Some("trending"),
            "window" => matches!(value.as_str(), Some("day" | "week")),
            "sortBy" => matches!(
                value.as_str(),
                Some("popularity.desc" | "vote_average.desc")
            ),
            "releaseWindow" => matches!(
                value.as_str(),
                Some(
                    "recent" | "theaters" | "opening-soon" | "upcoming" | "on-air" | "airing-soon"
                )
            ),
            "watchProvider" => matches!(
                value.as_str(),
                Some("netflix" | "prime-video" | "disney-plus" | "apple-tv-plus" | "max")
            ),
            "genre" => value.as_u64().is_some_and(|value| value > 0),
            "primaryReleaseYear" => value
                .as_i64()
                .is_some_and(|value| (1874..=2100).contains(&value)),
            "voteCountGte" => value.as_u64().is_some_and(|value| value <= 1_000_000),
            "language" | "originalLanguage" => value.as_str().is_some_and(valid_language),
            "region" => value.as_str().is_some_and(valid_region),
            "withKeywords" => value.as_str().is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 100
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'|' | b','))
            }),
            _ => false,
        };
        if !valid {
            return Err("the TMDB Discover parameters are invalid");
        }
    }
    Ok(())
}

fn valid_language(value: &str) -> bool {
    matches!(value.len(), 2..=16)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
}

fn valid_region(value: &str) -> bool {
    value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn source_schema_version() -> u32 {
    1
}

fn valid_mdblist_id(value: &str) -> bool {
    let value = value.trim();
    value.len() <= 160
        && !value.to_ascii_lowercase().contains("share")
        && match value.split('/').collect::<Vec<_>>().as_slice() {
            [single] => valid_mdblist_segment(single),
            [owner, name] => valid_mdblist_segment(owner) && valid_mdblist_segment(name),
            _ => false,
        }
}

fn valid_mdblist_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Tmdb,
    MdbList,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionProfile {
    pub id: String,
    pub revision: String,
    pub template: TemplateReference,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub custom_poster_id: Option<String>,
    pub source: CollectionSource,
    pub media_type: MediaType,
    #[serde(default)]
    pub limit: ResultLimit,
    #[serde(default)]
    pub ordering: ResultOrdering,
    #[serde(default)]
    pub cadence: RefreshCadence,
}

impl CollectionProfile {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_opaque_id(&self.id) || !valid_opaque_id(&self.revision) {
            return Err("the collection identity is invalid");
        }
        if self.template.id.trim().is_empty() || self.template.version == 0 {
            return Err("the originating template is invalid");
        }
        if self.title.trim().is_empty() || self.title.trim().chars().count() > 120 {
            return Err("the collection title must contain 1 to 120 characters");
        }
        if self.description.chars().count() > 2_000 {
            return Err("the collection description is too long");
        }
        if self
            .custom_poster_id
            .as_deref()
            .is_some_and(|id| !valid_opaque_id(id))
        {
            return Err("the custom poster identity is invalid");
        }
        if matches!(self.limit, ResultLimit::Maximum { count: 0 | 501.. }) {
            return Err("the collection result limit must be between 1 and 500");
        }
        if !self.source.supports_media_type(self.media_type) {
            return Err("the collection source does not support that media type");
        }
        if let CollectionSource::TmdbDiscover { parameters, .. } = &self.source
            && (parameters.contains_key("language") || parameters.contains_key("region"))
            && !self.template.id.ends_with(".custom-discover")
        {
            return Err("this template does not expose language or region overrides");
        }
        self.source.validate()
    }
}

pub fn valid_opaque_id(value: &str) -> bool {
    matches!(value.len(), 16..=64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanonicalIdentity {
    pub media_type: MediaType,
    pub tmdb_id: u64,
}

impl CanonicalIdentity {
    pub fn new(media_type: MediaType, tmdb_id: u64) -> Option<Self> {
        matches!(media_type, MediaType::Movie | MediaType::Series)
            .then_some(())
            .filter(|_| tmdb_id > 0)
            .map(|()| Self {
                media_type,
                tmdb_id,
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NormalizedTitle {
    #[serde(flatten)]
    pub identity: CanonicalIdentity,
    pub title: String,
    #[serde(default)]
    pub original_title: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub release_date: Option<String>,
    pub source_order: u32,
    #[serde(default)]
    pub poster_path: Option<String>,
    #[serde(default)]
    pub backdrop_path: Option<String>,
    #[serde(default)]
    pub adult: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectionSnapshot {
    pub profile_id: String,
    pub revision: String,
    pub committed_at: i64,
    pub items: Vec<NormalizedTitle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub played: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClassifiedTitle {
    #[serde(flatten)]
    pub title: NormalizedTitle,
    pub local_items: Vec<LocalItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClassifiedSnapshot {
    pub owned: Vec<ClassifiedTitle>,
    pub missing: Vec<NormalizedTitle>,
    pub items: Vec<NormalizedTitle>,
    pub ownership_available: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderReadiness {
    pub tmdb: bool,
    pub mdblist: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderResult {
    pub items: Vec<NormalizedTitle>,
    pub total: u32,
    pub movies: u32,
    pub series: u32,
    #[serde(default)]
    pub source_identity: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_identity_rejects_mixed_and_zero_ids() {
        assert!(CanonicalIdentity::new(MediaType::Movie, 603).is_some());
        assert!(CanonicalIdentity::new(MediaType::Mixed, 603).is_none());
        assert!(CanonicalIdentity::new(MediaType::Series, 0).is_none());
    }

    #[test]
    fn collection_source_is_a_camel_case_tagged_union() {
        let source = CollectionSource::TmdbCollection {
            schema_version: 1,
            collection_id: 10,
            include_unreleased: false,
        };
        let value = serde_json::to_value(source).expect("serialize");
        assert_eq!(value["kind"], "tmdbCollection");
        assert_eq!(value["collectionId"], 10);
    }

    #[test]
    fn discover_parameters_are_a_bounded_allowlist() {
        let valid = CollectionSource::TmdbDiscover {
            schema_version: 1,
            parameters: BTreeMap::from([
                ("language".to_string(), json!("de-DE")),
                ("region".to_string(), json!("DE")),
            ]),
        };
        assert!(valid.validate().is_ok());
        let invalid = CollectionSource::TmdbDiscover {
            schema_version: 1,
            parameters: BTreeMap::from([("url".to_string(), json!("https://example.com"))]),
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn every_source_accepts_only_media_types_its_provider_honors() {
        let discover = CollectionSource::TmdbDiscover {
            schema_version: 1,
            parameters: BTreeMap::new(),
        };
        let exact = CollectionSource::TmdbCollection {
            schema_version: 1,
            collection_id: 10,
            include_unreleased: false,
        };
        let mdblist = CollectionSource::MdbListPublicList {
            schema_version: 1,
            list_id: "42".to_string(),
        };

        assert!(discover.supports_media_type(MediaType::Movie));
        assert!(discover.supports_media_type(MediaType::Series));
        assert!(!discover.supports_media_type(MediaType::Mixed));
        assert!(exact.supports_media_type(MediaType::Movie));
        assert!(!exact.supports_media_type(MediaType::Series));
        assert!(!exact.supports_media_type(MediaType::Mixed));
        assert!(mdblist.supports_media_type(MediaType::Movie));
        assert!(mdblist.supports_media_type(MediaType::Series));
        assert!(mdblist.supports_media_type(MediaType::Mixed));
    }

    #[test]
    fn an_unknown_source_is_retained_and_disabled_without_losing_its_shape() {
        let raw = json!({
            "kind": "futureProvider",
            "schemaVersion": 7,
            "selector": { "opaque": true }
        });
        let source: CollectionSource = serde_json::from_value(raw.clone()).expect("source");
        assert!(source.validate().is_err());
        assert_eq!(serde_json::to_value(source).expect("retained source"), raw);
    }

    #[test]
    fn version_one_contract_fixtures_decode_to_the_domain_types() {
        let profile: CollectionProfile =
            serde_json::from_str(include_str!("../../fixtures/collections/profile-v1.json"))
                .expect("profile fixture");
        let template: crate::collections::templates::CollectionTemplate =
            serde_json::from_str(include_str!("../../fixtures/collections/template-v1.json"))
                .expect("template fixture");
        let snapshot: CollectionSnapshot =
            serde_json::from_str(include_str!("../../fixtures/collections/snapshot-v1.json"))
                .expect("snapshot fixture");
        let readiness: ProviderReadiness =
            serde_json::from_str(include_str!("../../fixtures/collections/readiness-v1.json"))
                .expect("readiness fixture");
        let result: ProviderResult = serde_json::from_str(include_str!(
            "../../fixtures/collections/provider-result-v1.json"
        ))
        .expect("provider fixture");

        assert!(profile.validate().is_ok());
        assert_eq!(template.provenance, profile.template);
        assert_eq!(snapshot.items[0], result.items[0]);
        assert!(readiness.tmdb);
        assert!(!readiness.mdblist);
        assert_eq!(result.source_identity.as_deref(), Some("fixture-v1"));
    }
}
