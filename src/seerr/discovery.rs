/// The discovery rows the UI can ask for. An enum rather than a string so a
/// path segment from the page can never reach the address unchecked.
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

    pub(super) fn path(self) -> &'static str {
        match self {
            Self::Trending => "discover/trending",
            Self::Movies => "discover/movies",
            Self::Tv => "discover/tv",
            Self::UpcomingMovies => "discover/movies/upcoming",
            Self::UpcomingTv => "discover/tv/upcoming",
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
    media_type: Option<TrendingMediaType>,
    time_window: Option<TrendingWindow>,
}

const EARLIEST_RELEASE_DECADE: u16 = 1900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UtcDate {
    pub(super) year: u16,
    pub(super) month: u8,
    pub(super) day: u8,
}

impl UtcDate {
    fn today() -> Self {
        let days = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() / 86_400)
            .unwrap_or_default();
        Self::from_unix_days(i64::try_from(days).unwrap_or_default())
    }

    // Gregorian civil date conversion by Howard Hinnant. Keeping this tiny
    // avoids adding a date-time dependency solely to cap one query parameter.
    pub(super) fn from_unix_days(days: i64) -> Self {
        let days = days + 719_468;
        let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let mut year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_prime = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
        let month = month_prime + if month_prime < 10 { 3 } else { -9 };
        year += i64::from(month <= 2);
        Self {
            year: u16::try_from(year).unwrap_or_default(),
            month: u8::try_from(month).unwrap_or_default(),
            day: u8::try_from(day).unwrap_or_default(),
        }
    }

    fn decade(self) -> u16 {
        self.year / 10 * 10
    }

    fn iso8601(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
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
    pub fn from_values(
        genre: Option<&str>,
        sort: Option<&str>,
        min_rating: Option<&str>,
        release_decade: Option<&str>,
        media_type: Option<&str>,
        time_window: Option<&str>,
    ) -> Result<Self, String> {
        let genre = genre
            .map(|value| {
                value
                    .parse::<i64>()
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or_else(|| "genre must be a positive number".to_string())
            })
            .transpose()?;
        let sort = sort
            .map(|value| match value {
                "popular" => Ok(DiscoverSort::Popular),
                "rating" => Ok(DiscoverSort::Rating),
                "newest" => Ok(DiscoverSort::Newest),
                _ => Err("unknown discovery sort".to_string()),
            })
            .transpose()?;
        let min_rating = min_rating
            .map(|value| {
                value
                    .parse::<u8>()
                    .ok()
                    .filter(|value| *value <= 10)
                    .ok_or_else(|| "minimum rating must be between 0 and 10".to_string())
            })
            .transpose()?;
        // Keep the public contract narrower than arbitrary upstream dates and
        // use the same twentieth-century boundary for films and television.
        let current_decade = UtcDate::today().decade();
        let release_decade = release_decade
            .map(|value| {
                value
                    .parse::<u16>()
                    .ok()
                    .filter(|value| {
                        value.is_multiple_of(10)
                            && (EARLIEST_RELEASE_DECADE..=current_decade).contains(value)
                    })
                    .ok_or_else(|| {
                        format!(
                            "release decade must be a ten-year start from {EARLIEST_RELEASE_DECADE} through {current_decade}"
                        )
                    })
            })
            .transpose()?;
        let media_type = media_type
            .map(|value| match value {
                "all" => Ok(TrendingMediaType::All),
                "movie" => Ok(TrendingMediaType::Movie),
                "tv" => Ok(TrendingMediaType::Tv),
                _ => Err("unknown trending media type".to_string()),
            })
            .transpose()?;
        let time_window = time_window
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
            media_type,
            time_window,
        })
    }

    /// Query pairs accepted by Seerr's documented discovery routes.
    pub fn query_pairs(&self, kind: DiscoverKind, page: i64) -> Vec<(&'static str, String)> {
        self.query_pairs_for(kind, page, false, UtcDate::today())
    }

    /// The Companion API keeps the application-level decade allowlist rather
    /// than exposing arbitrary upstream date strings. A current plugin expands
    /// this value to the same Seerr date pair as a direct session.
    pub fn companion_query_pairs(
        &self,
        kind: DiscoverKind,
        page: i64,
    ) -> Vec<(&'static str, String)> {
        self.query_pairs_for(kind, page, true, UtcDate::today())
    }

    pub(super) fn query_pairs_for(
        &self,
        kind: DiscoverKind,
        page: i64,
        companion: bool,
        today: UtcDate,
    ) -> Vec<(&'static str, String)> {
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
            DiscoverKind::Movies | DiscoverKind::Tv => {
                if let Some(genre) = self.genre {
                    query.push(("genre", genre.to_string()));
                }
                if let Some(decade) = self.release_decade {
                    if companion {
                        query.push(("releaseDecade", decade.to_string()));
                    } else {
                        let upper_bound = if decade == today.decade() {
                            today.iso8601()
                        } else {
                            format!("{}-12-31", decade + 9)
                        };
                        let (gte, lte) = match kind {
                            DiscoverKind::Movies => {
                                ("primaryReleaseDateGte", "primaryReleaseDateLte")
                            }
                            DiscoverKind::Tv => ("firstAirDateGte", "firstAirDateLte"),
                            _ => unreachable!(),
                        };
                        query.push((gte, format!("{decade:04}-01-01")));
                        query.push((lte, upper_bound));
                    }
                }
                if let Some(sort) = self.sort {
                    let value = match (sort, kind) {
                        (DiscoverSort::Popular, _) => "popularity.desc",
                        (DiscoverSort::Rating, _) => "vote_average.desc",
                        (DiscoverSort::Newest, DiscoverKind::Movies) => "primary_release_date.desc",
                        (DiscoverSort::Newest, DiscoverKind::Tv) => "first_air_date.desc",
                        (DiscoverSort::Newest, _) => unreachable!(),
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
            DiscoverKind::UpcomingMovies | DiscoverKind::UpcomingTv => {}
        }

        query
    }
}
