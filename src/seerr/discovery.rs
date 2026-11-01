use crate::library::{EARLIEST_RELEASE_DECADE, current_release_decade};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverKind {
    Trending,
    Movies,
    Tv,
    UpcomingMovies,
    UpcomingTv,
}

impl DiscoverKind {
    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "trending" => Some(Self::Trending),
            "movies" => Some(Self::Movies),
            "tv" => Some(Self::Tv),
            "upcoming-movies" => Some(Self::UpcomingMovies),
            "upcoming-tv" => Some(Self::UpcomingTv),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Trending => "trending",
            Self::Movies => "movies",
            Self::Tv => "tv",
            Self::UpcomingMovies => "upcoming-movies",
            Self::UpcomingTv => "upcoming-tv",
        }
    }
}

/// The small, allowlisted set of Seerr discovery controls exposed by the UI.
///
/// Keeping these as application-level names means neither the app scheme nor
/// the Companion plugin becomes a general query-string proxy to Seerr.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoverOptions {
    genre: Option<i64>,
    sort: Option<DiscoverSort>,
    min_rating: Option<u8>,
    release_decade: Option<u16>,
    /// A TMDB company id; the movie catalog only.
    studio: Option<i64>,
    /// A TMDB network id; the TV catalog only.
    network: Option<i64>,
    media_type: Option<TrendingMediaType>,
    time_window: Option<TrendingWindow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoverSort {
    Popular,
    Rating,
    Newest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrendingMediaType {
    All,
    Movie,
    Tv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrendingWindow {
    Day,
    Week,
}

impl DiscoverOptions {
    /// Reads the UI's discovery query parameters, by name, from `param`.
    pub fn from_query(param: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        // TMDB ids, which the Companion reads as 32-bit integers.
        let positive = |name: &str, message: &str| {
            param(name)
                .map(|value| {
                    value
                        .parse::<i32>()
                        .ok()
                        .filter(|value| *value > 0)
                        .map(i64::from)
                        .ok_or_else(|| message.to_string())
                })
                .transpose()
        };
        let genre = positive("genre", "genre must be a positive number")?;
        let studio = positive("studio", "studio must be a positive TMDB company id")?;
        let network = positive("network", "network must be a positive TMDB network id")?;
        let sort = param("sort")
            .as_deref()
            .map(|value| match value {
                "popular" => Ok(DiscoverSort::Popular),
                "rating" => Ok(DiscoverSort::Rating),
                "newest" => Ok(DiscoverSort::Newest),
                _ => Err("unknown discovery sort".to_string()),
            })
            .transpose()?;
        let min_rating = param("minRating")
            .as_deref()
            .map(|value| {
                value
                    .parse::<u8>()
                    .ok()
                    .filter(|value| *value <= 10)
                    .ok_or_else(|| "minimum rating must be between 0 and 10".to_string())
            })
            .transpose()?;
        let current_decade = current_release_decade();
        let release_decade = param("decade")
            .as_deref()
            .map(|value| {
                value
                    .parse::<u16>()
                    .ok()
                    .filter(|value| {
                        value.is_multiple_of(10)
                            && (EARLIEST_RELEASE_DECADE..=current_decade)
                                .contains(&i64::from(*value))
                    })
                    .ok_or_else(|| {
                        format!(
                            "release decade must be a ten-year start from {EARLIEST_RELEASE_DECADE} through {current_decade}"
                        )
                    })
            })
            .transpose()?;
        let media_type = param("mediaType")
            .as_deref()
            .map(|value| match value {
                "all" => Ok(TrendingMediaType::All),
                "movie" => Ok(TrendingMediaType::Movie),
                "tv" => Ok(TrendingMediaType::Tv),
                _ => Err("unknown trending media type".to_string()),
            })
            .transpose()?;
        let time_window = param("timeWindow")
            .as_deref()
            .map(|value| match value {
                "day" => Ok(TrendingWindow::Day),
                "week" => Ok(TrendingWindow::Week),
                _ => Err("unknown trending time window".to_string()),
            })
            .transpose()?;

        Ok(Self {
            genre,
            sort,
            min_rating,
            release_decade,
            studio,
            network,
            media_type,
            time_window,
        })
    }

    pub fn query_pairs(&self, kind: DiscoverKind, page: i64) -> Vec<(&'static str, String)> {
        let mut query = vec![("page", page.clamp(1, 1_000).to_string())];

        match kind {
            DiscoverKind::Trending => {
                if let Some(media_type) = self.media_type {
                    query.push((
                        "mediaType",
                        match media_type {
                            TrendingMediaType::All => "all",
                            TrendingMediaType::Movie => "movie",
                            TrendingMediaType::Tv => "tv",
                        }
                        .to_string(),
                    ));
                }
                if let Some(time_window) = self.time_window {
                    query.push((
                        "timeWindow",
                        match time_window {
                            TrendingWindow::Day => "day",
                            TrendingWindow::Week => "week",
                        }
                        .to_string(),
                    ));
                }
            }
            DiscoverKind::Movies => {
                self.push_catalog_filters(&mut query, "primary_release_date.desc");
                if let Some(studio) = self.studio {
                    query.push(("studio", studio.to_string()));
                }
            }
            DiscoverKind::Tv => {
                self.push_catalog_filters(&mut query, "first_air_date.desc");
                if let Some(network) = self.network {
                    query.push(("network", network.to_string()));
                }
            }
            DiscoverKind::UpcomingMovies | DiscoverKind::UpcomingTv => {}
        }

        query
    }

    /// The movie and TV catalogs take the same filters and differ only in the
    /// field TMDB sorts by for "newest".
    fn push_catalog_filters(
        &self,
        query: &mut Vec<(&'static str, String)>,
        newest_sort: &'static str,
    ) {
        if let Some(genre) = self.genre {
            query.push(("genre", genre.to_string()));
        }
        if let Some(decade) = self.release_decade {
            query.push(("releaseDecade", decade.to_string()));
        }
        if let Some(sort) = self.sort {
            let value = match sort {
                DiscoverSort::Popular => "popularity.desc",
                DiscoverSort::Rating => "vote_average.desc",
                DiscoverSort::Newest => newest_sort,
            };
            query.push(("sortBy", value.to_string()));
            // TMDB's vote-average sort otherwise promotes titles with
            // a single perfect vote above established favourites.
            if sort == DiscoverSort::Rating {
                query.push(("voteCountGte", "50".to_string()));
            }
        }
        if let Some(min_rating) = self.min_rating {
            query.push(("voteAverageGte", min_rating.to_string()));
        }
    }
}
