use serde::{Serialize, Serializer};

use crate::jellyfin::api::model::UserViewDto;

use super::SortKey;
use super::model::TitleKind;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct LibraryId(String);

impl LibraryId {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        crate::app::ids::valid_opaque_id(raw).then(|| Self(raw.to_ascii_lowercase()))
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
    Catalog { title: TitleKind },
    Live,
}

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

pub static CROSS_LIBRARY_BROWSE: BrowseCapabilities = BrowseCapabilities {
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
    pub(super) collection_type: Option<String>,
    pub primary_image_tag: Option<String>,
    pub membership_pending: bool,
}

impl MediaLibrary {
    pub fn from_view(view: &UserViewDto) -> Option<Self> {
        Some(Self {
            id: LibraryId::parse(&view.id)?,
            name: view.name.clone().unwrap_or_default(),
            collection_type: view.collection_type.clone(),
            primary_image_tag: view.primary_image_tag().map(str::to_string),
            membership_pending: false,
        })
    }

    pub fn kind(&self) -> LibraryKind {
        LibraryKind::from_collection_type(self.collection_type.as_deref())
    }

    pub fn catalog_title(&self) -> Option<TitleKind> {
        match self.kind().profile().grid {
            GridSource::Catalog { title } => Some(title),
            GridSource::Live => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRegistry {
    Unknown,
    Known(Vec<MediaLibrary>),
}
