//! The Jellyfin libraries a user sees, and how each kind of library is served.

use serde::{Serialize, Serializer};

use crate::jellyfin::api::model::UserViewDto;

use super::SortKey;
use super::model::TitleKind;

/// A Jellyfin library id: validated hex, so it is safe as a route segment and
/// a SQL argument.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct LibraryId(String);

impl LibraryId {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        crate::collections::valid_opaque_id(raw).then(|| Self(raw.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryKind {
    Movies,
    Shows,
    Music,
    MusicVideos,
    HomeVideos,
    Photos,
    Books,
    BoxSets,
    Playlists,
    LiveTv,
    Trailers,
    Mixed,
}

impl LibraryKind {
    /// The only reader of Jellyfin's CollectionType. No value, `folders`, and
    /// any value a newer server adds read as `Mixed`, so every library is
    /// still listed.
    pub fn from_collection_type(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("movies") => Self::Movies,
            Some("tvshows") => Self::Shows,
            Some("music") => Self::Music,
            Some("musicvideos") => Self::MusicVideos,
            Some("homevideos") => Self::HomeVideos,
            Some("photos") => Self::Photos,
            Some("books") => Self::Books,
            Some("boxsets") => Self::BoxSets,
            Some("playlists") => Self::Playlists,
            Some("livetv") => Self::LiveTv,
            Some("trailers") => Self::Trailers,
            _ => Self::Mixed,
        }
    }

    /// How each kind is served. Exhaustive: a new kind without a row does not
    /// build.
    pub fn profile(self) -> LibraryProfile {
        match self {
            Self::Movies => LibraryProfile::catalog(TitleKind::Movie, &MOVIE_BROWSE),
            Self::Shows => LibraryProfile::catalog(TitleKind::Series, &SHOW_BROWSE),
            Self::Music | Self::MusicVideos | Self::Books | Self::BoxSets | Self::Trailers => {
                LibraryProfile::live(&LIVE_RECURSIVE_BROWSE)
            }
            Self::HomeVideos | Self::Photos | Self::Playlists | Self::LiveTv | Self::Mixed => {
                LibraryProfile::live(&LIVE_FOLDER_BROWSE)
            }
        }
    }
}

pub struct LibraryProfile {
    pub grid: GridSource,
    pub browse: &'static BrowseCapabilities,
}

impl LibraryProfile {
    const fn catalog(title: TitleKind, browse: &'static BrowseCapabilities) -> Self {
        Self {
            grid: GridSource::Catalog { title },
            browse,
        }
    }

    const fn live(browse: &'static BrowseCapabilities) -> Self {
        Self {
            grid: GridSource::Live,
            browse,
        }
    }
}

pub enum GridSource {
    /// The cached catalog, scoped by `item_libraries`. `title` is the one kind
    /// the grid shows and the membership pass walks.
    Catalog { title: TitleKind },
    /// Never cached: its grid comes from Jellyfin, which this version does
    /// not browse yet.
    Live,
}

/// A grid filter. Its id is also the request parameter that sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKey {
    Genre,
    Decade,
    Watched,
    Parental,
    SeriesStatus,
    Favorite,
}

impl FilterKey {
    pub fn id(self) -> &'static str {
        match self {
            Self::Genre => "genre",
            Self::Decade => "decade",
            Self::Watched => "watched",
            Self::Parental => "parental",
            Self::SeriesStatus => "seriesStatus",
            Self::Favorite => "favorite",
        }
    }
}

impl Serialize for FilterKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.id())
    }
}

/// What a grid can run. A request for anything else is a 400.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseCapabilities {
    pub sorts: &'static [SortKey],
    pub filters: &'static [FilterKey],
    pub letter_index: bool,
}

pub static MOVIE_BROWSE: BrowseCapabilities = BrowseCapabilities {
    sorts: &[
        SortKey::Name,
        SortKey::ReleaseDate,
        SortKey::DateAdded,
        SortKey::CommunityRating,
        SortKey::DatePlayed,
        SortKey::PlayCount,
        SortKey::Runtime,
        SortKey::Random,
    ],
    filters: &[
        FilterKey::Genre,
        FilterKey::Decade,
        FilterKey::Watched,
        FilterKey::Parental,
        FilterKey::Favorite,
    ],
    letter_index: true,
};

pub static SHOW_BROWSE: BrowseCapabilities = BrowseCapabilities {
    sorts: &[
        SortKey::Name,
        SortKey::ReleaseDate,
        SortKey::DateAdded,
        SortKey::EpisodeAdded,
        SortKey::CommunityRating,
        SortKey::DatePlayed,
        SortKey::Random,
    ],
    filters: &[
        FilterKey::Genre,
        FilterKey::Decade,
        FilterKey::Watched,
        FilterKey::Parental,
        FilterKey::SeriesStatus,
        FilterKey::Favorite,
    ],
    letter_index: true,
};

/// The cross-library title views: Search, Favorites, a person, a genre.
pub static TITLE_BROWSE: BrowseCapabilities = BrowseCapabilities {
    sorts: &[
        SortKey::Name,
        SortKey::ReleaseDate,
        SortKey::DateAdded,
        SortKey::CommunityRating,
        SortKey::DatePlayed,
        SortKey::Random,
    ],
    filters: &[
        FilterKey::Genre,
        FilterKey::Decade,
        FilterKey::Watched,
        FilterKey::Parental,
        FilterKey::Favorite,
    ],
    letter_index: true,
};

/// No random sort: Jellyfin's is unseeded, so its pages would overlap.
pub static LIVE_RECURSIVE_BROWSE: BrowseCapabilities = BrowseCapabilities {
    sorts: &[SortKey::Name, SortKey::DateAdded, SortKey::ReleaseDate],
    filters: &[FilterKey::Favorite],
    letter_index: false,
};

pub static LIVE_FOLDER_BROWSE: BrowseCapabilities = BrowseCapabilities {
    sorts: &[SortKey::Name, SortKey::DateAdded, SortKey::ReleaseDate],
    filters: &[],
    letter_index: false,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaLibrary {
    pub id: LibraryId,
    pub name: String,
    /// Jellyfin's CollectionType as received. `kind()` maps it.
    pub(super) collection_type: Option<String>,
    pub primary_image_tag: Option<String>,
}

impl MediaLibrary {
    /// `None` for a view whose id is not one this app can address.
    pub fn from_view(view: &UserViewDto) -> Option<Self> {
        Some(Self {
            id: LibraryId::parse(&view.id)?,
            name: view.name.clone().unwrap_or_default(),
            collection_type: view.collection_type.clone(),
            primary_image_tag: view.primary_image_tag().map(str::to_string),
        })
    }

    pub fn kind(&self) -> LibraryKind {
        LibraryKind::from_collection_type(self.collection_type.as_deref())
    }

    /// The kind a catalog library's grid lists; `None` for a library served
    /// live.
    pub fn catalog_title(&self) -> Option<TitleKind> {
        match self.kind().profile().grid {
            GridSource::Catalog { title } => Some(title),
            GridSource::Live => None,
        }
    }
}

/// `Unknown`: the server's library list was never read into this cache.
/// `Known(vec![])`: the server says the user sees no library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRegistry {
    Unknown,
    Known(Vec<MediaLibrary>),
}

impl LibraryRegistry {
    pub fn get(&self, id: &LibraryId) -> Option<&MediaLibrary> {
        match self {
            Self::Unknown => None,
            Self::Known(libraries) => libraries.iter().find(|library| library.id == *id),
        }
    }
}
