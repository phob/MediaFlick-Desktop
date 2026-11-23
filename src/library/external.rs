use serde::{Deserialize, Serialize};

use crate::app::urls::encode_query_component;

use super::model::{ItemDetail, ProviderIds};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum ExternalProvider {
    Imdb,
    Tmdb,
    Tvdb,
    Letterboxd,
    Trakt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
pub enum ExternalLinkId {
    #[serde(rename = "imdb")]
    Imdb,
    #[serde(rename = "tmdb")]
    Tmdb,
    #[serde(rename = "tvdb")]
    Tvdb,
    #[serde(rename = "letterboxd")]
    Letterboxd,
    #[serde(rename = "trakt")]
    Trakt,
    #[serde(rename = "rotten-tomatoes-search")]
    RottenTomatoesSearch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExternalLink {
    Provider {
        id: ExternalProvider,
        label: String,
    },
    Url {
        id: ExternalLinkId,
        label: String,
        href: String,
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        action_label: Option<String>,
    },
}

impl ExternalProvider {
    pub const ALL: [Self; 5] = [
        Self::Imdb,
        Self::Tmdb,
        Self::Tvdb,
        Self::Letterboxd,
        Self::Trakt,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Imdb => "IMDb",
            Self::Tmdb => "TMDB",
            Self::Tvdb => "TVDB",
            Self::Letterboxd => "Letterboxd",
            Self::Trakt => "Trakt",
        }
    }

    pub const fn link_id(self) -> ExternalLinkId {
        match self {
            Self::Imdb => ExternalLinkId::Imdb,
            Self::Tmdb => ExternalLinkId::Tmdb,
            Self::Tvdb => ExternalLinkId::Tvdb,
            Self::Letterboxd => ExternalLinkId::Letterboxd,
            Self::Trakt => ExternalLinkId::Trakt,
        }
    }

    pub const fn id_field(self) -> &'static str {
        match self {
            Self::Imdb | Self::Trakt => "imdb",
            Self::Tmdb | Self::Letterboxd => "tmdb",
            Self::Tvdb => "tvdb",
        }
    }

    pub fn url(self, id: &str, kind: &str) -> Option<String> {
        if !valid_external_id(self.id_field(), id) {
            return None;
        }
        Some(match (self, kind) {
            (Self::Imdb, "Movie" | "Series" | "Episode") => {
                format!("https://www.imdb.com/title/{id}/")
            }
            (Self::Tmdb, "Movie") => format!("https://www.themoviedb.org/movie/{id}"),
            (Self::Tmdb, "Series") => format!("https://www.themoviedb.org/tv/{id}"),
            (Self::Tvdb, "Movie") => format!("https://thetvdb.com/dereferrer/movie/{id}"),
            (Self::Tvdb, "Series") => format!("https://thetvdb.com/dereferrer/series/{id}"),
            (Self::Tvdb, "Episode") => format!("https://thetvdb.com/dereferrer/episode/{id}"),
            (Self::Letterboxd, "Movie") => format!("https://letterboxd.com/tmdb/{id}"),
            (Self::Trakt, "Movie") => format!("https://trakt.tv/movies/{id}"),
            (Self::Trakt, "Series") => format!("https://trakt.tv/shows/{id}"),
            _ => return None,
        })
    }
}

fn valid_external_id(source: &str, id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    match source {
        "imdb" => id.strip_prefix("tt").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        }),
        "tmdb" | "tvdb" => {
            id.bytes().all(|byte| byte.is_ascii_digit()) && id.bytes().any(|byte| byte != b'0')
        }
        _ => false,
    }
}

fn rotten_tomatoes_search_link(title: &str, year: Option<i64>) -> Option<ExternalLink> {
    let normalized_title = title.trim();
    if normalized_title.is_empty() {
        return None;
    }
    let query = match year {
        Some(year) if year > 1800 => format!("{normalized_title} {year}"),
        _ => normalized_title.to_string(),
    };
    Some(ExternalLink::Url {
        id: ExternalLinkId::RottenTomatoesSearch,
        label: "Rotten Tomatoes".to_string(),
        href: format!(
            "https://www.rottentomatoes.com/search?search={}",
            encode_query_component(&query)
        ),
        action_label: Some("Search Rotten Tomatoes".to_string()),
    })
}

pub fn external_links_for_item(item: &ItemDetail) -> Vec<ExternalLink> {
    let kind = item.summary.kind.as_str();
    let mut links = ExternalProvider::ALL
        .into_iter()
        .filter_map(|provider| {
            let id = item.provider_ids.get(provider.id_field())?;
            provider.url(id, kind)?;
            Some(ExternalLink::Provider {
                id: provider,
                label: provider.label().to_string(),
            })
        })
        .collect::<Vec<_>>();
    if matches!(kind, "Movie" | "Series")
        && let Some(search) = rotten_tomatoes_search_link(&item.summary.name, item.summary.year)
    {
        links.push(search);
    }
    links
}

pub fn external_links_for_discovery(
    kind: &str,
    title: &str,
    year: Option<i64>,
    ids: &ProviderIds,
) -> Vec<ExternalLink> {
    let mut links = ExternalProvider::ALL
        .into_iter()
        .filter_map(|provider| {
            let id = ids.get(provider.id_field())?;
            let href = provider.url(id, kind)?;
            Some(ExternalLink::Url {
                id: provider.link_id(),
                label: provider.label().to_string(),
                href,
                action_label: None,
            })
        })
        .collect::<Vec<_>>();
    if matches!(kind, "Movie" | "Series")
        && let Some(search) = rotten_tomatoes_search_link(title, year)
    {
        links.push(search);
    }
    links
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalLink, ExternalLinkId, ExternalProvider, external_links_for_discovery,
        external_links_for_item,
    };
    use crate::library::model::{ItemDetail, ItemSummary, ProviderIds};

    fn detail(kind: &str, name: &str, year: Option<i64>, provider_ids: ProviderIds) -> ItemDetail {
        ItemDetail {
            summary: ItemSummary {
                id: "item-1".to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                year,
                ..ItemSummary::default()
            },
            provider_ids,
            ..ItemDetail::default()
        }
    }

    #[test]
    fn provider_urls_match_allowed_kinds_and_templates() {
        assert_eq!(
            ExternalProvider::Imdb.url("tt0133093", "Movie").as_deref(),
            Some("https://www.imdb.com/title/tt0133093/")
        );
        assert_eq!(
            ExternalProvider::Imdb
                .url("tt11280740", "Series")
                .as_deref(),
            Some("https://www.imdb.com/title/tt11280740/")
        );
        assert_eq!(
            ExternalProvider::Imdb
                .url("tt13407582", "Episode")
                .as_deref(),
            Some("https://www.imdb.com/title/tt13407582/")
        );
        assert_eq!(
            ExternalProvider::Tmdb.url("603", "Movie").as_deref(),
            Some("https://www.themoviedb.org/movie/603")
        );
        assert_eq!(
            ExternalProvider::Tmdb.url("95396", "Series").as_deref(),
            Some("https://www.themoviedb.org/tv/95396")
        );
        assert_eq!(
            ExternalProvider::Tvdb.url("169", "Movie").as_deref(),
            Some("https://thetvdb.com/dereferrer/movie/169")
        );
        assert_eq!(
            ExternalProvider::Tvdb.url("371980", "Series").as_deref(),
            Some("https://thetvdb.com/dereferrer/series/371980")
        );
        assert_eq!(
            ExternalProvider::Tvdb.url("371980", "Episode").as_deref(),
            Some("https://thetvdb.com/dereferrer/episode/371980")
        );
        assert_eq!(
            ExternalProvider::Letterboxd.url("603", "Movie").as_deref(),
            Some("https://letterboxd.com/tmdb/603")
        );
        assert_eq!(
            ExternalProvider::Trakt.url("tt0133093", "Movie").as_deref(),
            Some("https://trakt.tv/movies/tt0133093")
        );
        assert_eq!(
            ExternalProvider::Trakt
                .url("tt11280740", "Series")
                .as_deref(),
            Some("https://trakt.tv/shows/tt11280740")
        );
    }

    #[test]
    fn provider_urls_reject_unsupported_kinds_and_malformed_ids() {
        assert_eq!(ExternalProvider::Tmdb.url("12345", "Episode"), None);
        assert_eq!(ExternalProvider::Tmdb.url("12345", "Season"), None);
        assert_eq!(ExternalProvider::Tvdb.url("12345", "Season"), None);
        assert_eq!(ExternalProvider::Imdb.url("tt0133093", "Season"), None);
        assert_eq!(ExternalProvider::Letterboxd.url("95396", "Series"), None);
        assert_eq!(ExternalProvider::Letterboxd.url("95396", "Episode"), None);
        assert_eq!(ExternalProvider::Trakt.url("tt0133093", "Episode"), None);
        assert_eq!(ExternalProvider::Imdb.url("", "Movie"), None);
        assert_eq!(ExternalProvider::Trakt.url("12345", "Movie"), None);

        let too_long = format!("tt{}", "1".repeat(31));
        for bad in [
            "tt1/../evil",
            "603?x=1",
            "603#f",
            "603 604",
            "https://example.test",
            &too_long,
        ] {
            assert_eq!(ExternalProvider::Imdb.url(bad, "Movie"), None, "{bad}");
        }
        assert_eq!(ExternalProvider::Imdb.url("603", "Movie"), None);
        assert_eq!(ExternalProvider::Tmdb.url("tt0133093", "Movie"), None);
        assert_eq!(ExternalProvider::Tmdb.url("0", "Movie"), None);
        assert_eq!(ExternalProvider::Tmdb.url("000", "Movie"), None);
        assert_eq!(ExternalProvider::Tvdb.url("-1", "Series"), None);
    }

    #[test]
    fn library_movies_emit_provider_actions_and_rotten_tomatoes_search() {
        let item = detail(
            "Movie",
            "The Matrix",
            Some(1999),
            ProviderIds {
                tmdb: Some("603".to_string()),
                imdb: Some("tt0133093".to_string()),
                tvdb: Some("169".to_string()),
            },
        );
        assert_eq!(
            external_links_for_item(&item),
            vec![
                ExternalLink::Provider {
                    id: ExternalProvider::Imdb,
                    label: "IMDb".to_string(),
                },
                ExternalLink::Provider {
                    id: ExternalProvider::Tmdb,
                    label: "TMDB".to_string(),
                },
                ExternalLink::Provider {
                    id: ExternalProvider::Tvdb,
                    label: "TVDB".to_string(),
                },
                ExternalLink::Provider {
                    id: ExternalProvider::Letterboxd,
                    label: "Letterboxd".to_string(),
                },
                ExternalLink::Provider {
                    id: ExternalProvider::Trakt,
                    label: "Trakt".to_string(),
                },
                ExternalLink::Url {
                    id: ExternalLinkId::RottenTomatoesSearch,
                    label: "Rotten Tomatoes".to_string(),
                    href: "https://www.rottentomatoes.com/search?search=The%20Matrix%201999"
                        .to_string(),
                    action_label: Some("Search Rotten Tomatoes".to_string()),
                },
            ]
        );
    }

    #[test]
    fn library_episodes_and_invalid_ids_omit_unsupported_links() {
        let episode = detail(
            "Episode",
            "Good News About Hell",
            Some(2022),
            ProviderIds {
                tmdb: Some("95396".to_string()),
                imdb: Some("tt13407582".to_string()),
                tvdb: Some("8888".to_string()),
            },
        );
        assert_eq!(
            external_links_for_item(&episode),
            vec![
                ExternalLink::Provider {
                    id: ExternalProvider::Imdb,
                    label: "IMDb".to_string(),
                },
                ExternalLink::Provider {
                    id: ExternalProvider::Tvdb,
                    label: "TVDB".to_string(),
                },
            ]
        );

        let invalid = detail(
            "Series",
            "Severance",
            Some(2022),
            ProviderIds {
                tmdb: Some("0".to_string()),
                imdb: Some("603?title=wrong".to_string()),
                tvdb: Some("not-a-number".to_string()),
            },
        );
        assert_eq!(
            external_links_for_item(&invalid),
            vec![ExternalLink::Url {
                id: ExternalLinkId::RottenTomatoesSearch,
                label: "Rotten Tomatoes".to_string(),
                href: "https://www.rottentomatoes.com/search?search=Severance%202022".to_string(),
                action_label: Some("Search Rotten Tomatoes".to_string()),
            }]
        );

        let blank_title = detail("Movie", "   ", Some(1700), ProviderIds::default());
        assert_eq!(external_links_for_item(&blank_title), Vec::new());
    }

    #[test]
    fn discovery_links_emit_validated_urls_for_movies_and_series() {
        let movie = external_links_for_discovery(
            "Movie",
            "The Matrix",
            Some(1999),
            &ProviderIds {
                tmdb: Some("603".to_string()),
                imdb: Some("tt0133093".to_string()),
                tvdb: None,
            },
        );
        assert_eq!(
            movie,
            vec![
                ExternalLink::Url {
                    id: ExternalLinkId::Imdb,
                    label: "IMDb".to_string(),
                    href: "https://www.imdb.com/title/tt0133093/".to_string(),
                    action_label: None,
                },
                ExternalLink::Url {
                    id: ExternalLinkId::Tmdb,
                    label: "TMDB".to_string(),
                    href: "https://www.themoviedb.org/movie/603".to_string(),
                    action_label: None,
                },
                ExternalLink::Url {
                    id: ExternalLinkId::Letterboxd,
                    label: "Letterboxd".to_string(),
                    href: "https://letterboxd.com/tmdb/603".to_string(),
                    action_label: None,
                },
                ExternalLink::Url {
                    id: ExternalLinkId::Trakt,
                    label: "Trakt".to_string(),
                    href: "https://trakt.tv/movies/tt0133093".to_string(),
                    action_label: None,
                },
                ExternalLink::Url {
                    id: ExternalLinkId::RottenTomatoesSearch,
                    label: "Rotten Tomatoes".to_string(),
                    href: "https://www.rottentomatoes.com/search?search=The%20Matrix%201999"
                        .to_string(),
                    action_label: Some("Search Rotten Tomatoes".to_string()),
                },
            ]
        );

        let series = external_links_for_discovery(
            "Series",
            "Severance",
            None,
            &ProviderIds {
                tmdb: Some("95396".to_string()),
                imdb: Some("https://example.test".to_string()),
                tvdb: Some("-1".to_string()),
            },
        );
        assert_eq!(
            series,
            vec![
                ExternalLink::Url {
                    id: ExternalLinkId::Tmdb,
                    label: "TMDB".to_string(),
                    href: "https://www.themoviedb.org/tv/95396".to_string(),
                    action_label: None,
                },
                ExternalLink::Url {
                    id: ExternalLinkId::RottenTomatoesSearch,
                    label: "Rotten Tomatoes".to_string(),
                    href: "https://www.rottentomatoes.com/search?search=Severance".to_string(),
                    action_label: Some("Search Rotten Tomatoes".to_string()),
                },
            ]
        );
    }
}
