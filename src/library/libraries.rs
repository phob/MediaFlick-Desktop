use serde::{Deserialize, Serialize, Serializer};

use crate::jellyfin::api::model::UserViewDto;

use super::SortKey;
use super::model::TitleKind;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct LibraryId(String);

impl LibraryId {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        crate::app::ids::valid_opaque_id(raw).then(|| Self(raw.to_ascii_lowercase()))
    }

    pub fn canonical(raw: &str) -> Option<Self> {
        Self::parse(raw).filter(|id| id.as_str() == raw)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for LibraryId {
    type Error = &'static str;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::canonical(&raw).ok_or("invalid library id")
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

pub const PLAYLIST_TYPE: &str = "Playlist";

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
        use LiveLatest::{Grouped, Newest};
        let recursive = |item_types: &'static [&'static str]| LiveShape::Recursive { item_types };
        match self {
            Self::Movies => LibraryProfile::catalog(TitleKind::Movie, &MOVIE_BROWSE),
            Self::Shows => LibraryProfile::catalog(TitleKind::Series, &SHOW_BROWSE),
            Self::Music => LibraryProfile::live(recursive(&["MusicAlbum"]), Grouped),
            Self::MusicVideos => LibraryProfile::live(recursive(&["MusicVideo"]), Grouped),
            Self::Books => LibraryProfile::live(recursive(&["Book", "AudioBook"]), Grouped),
            Self::BoxSets => LibraryProfile::live(recursive(&["BoxSet"]), Newest),
            Self::Trailers => LibraryProfile::live(recursive(&["Trailer"]), Grouped),
            Self::HomeVideos | Self::Photos => LibraryProfile::live(LiveShape::Folder, Grouped),
            Self::Playlists | Self::LiveTv | Self::Mixed => {
                LibraryProfile::live(LiveShape::Folder, Newest)
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

    const fn live(shape: LiveShape, latest: LiveLatest) -> Self {
        Self {
            grid: GridSource::Live { shape, latest },
            browse: match shape {
                LiveShape::Recursive { .. } => &LIVE_RECURSIVE_BROWSE,
                LiveShape::Folder => &LIVE_FOLDER_BROWSE,
            },
        }
    }
}

pub enum GridSource {
    Catalog {
        title: TitleKind,
    },
    Live {
        shape: LiveShape,
        latest: LiveLatest,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveShape {
    Recursive { item_types: &'static [&'static str] },
    Folder,
}

/// Where "Latest in <library>" comes from. The table follows jellyfin-web,
/// which asks `/Items/Latest` for every library except the view types it
/// excludes: `playlists`, `livetv`, `boxsets`, `channels` and `folders`.
/// Measured with the test servers: music (Grouped, demo), playlists (Newest,
/// demo) and box sets (Newest, own server); the other kinds are unmeasured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveLatest {
    /// Jellyfin's `/Items/Latest`, which groups new tracks into their album.
    Grouped,
    /// The library's own grid, newest first. `/Items/Latest` answers nothing
    /// for box sets and playlists.
    Newest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveSort {
    Name,
    DateAdded,
    ReleaseDate,
}

impl LiveSort {
    const ALL: [Self; 3] = [Self::Name, Self::DateAdded, Self::ReleaseDate];

    const fn key(self) -> SortKey {
        match self {
            Self::Name => SortKey::Name,
            Self::DateAdded => SortKey::DateAdded,
            Self::ReleaseDate => SortKey::ReleaseDate,
        }
    }

    pub fn from_key(key: SortKey) -> Option<Self> {
        Self::ALL.into_iter().find(|sort| sort.key() == key)
    }
}

const LIVE_SORT_KEYS: [SortKey; LiveSort::ALL.len()] = {
    let mut keys = [SortKey::Name; LiveSort::ALL.len()];
    let mut index = 0;
    while index < keys.len() {
        keys[index] = LiveSort::ALL[index].key();
        index += 1;
    }
    keys
};

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
    sorts: &LIVE_SORT_KEYS,
    filters: &[FilterKey::Favorite],
    letter_index: false,
};

pub static LIVE_FOLDER_BROWSE: BrowseCapabilities = BrowseCapabilities {
    sorts: &LIVE_SORT_KEYS,
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
            GridSource::Live { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRegistry {
    Unknown,
    Known(Vec<MediaLibrary>),
}
