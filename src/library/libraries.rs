//! The Jellyfin libraries a user sees, and how each kind of library is served.

use serde::Serialize;

use crate::jellyfin::api::model::UserViewDto;

use super::model::CatalogKind;

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
            Self::Movies => LibraryProfile::catalog(CatalogKind::Movie),
            Self::Shows => LibraryProfile::catalog(CatalogKind::Series),
            Self::Music
            | Self::MusicVideos
            | Self::Books
            | Self::BoxSets
            | Self::Trailers
            | Self::HomeVideos
            | Self::Photos
            | Self::Playlists
            | Self::LiveTv
            | Self::Mixed => LibraryProfile::live(),
        }
    }
}

pub struct LibraryProfile {
    pub grid: GridSource,
}

impl LibraryProfile {
    const fn catalog(title: CatalogKind) -> Self {
        Self {
            grid: GridSource::Catalog { title },
        }
    }

    const fn live() -> Self {
        Self {
            grid: GridSource::Live,
        }
    }
}

pub enum GridSource {
    /// The cached catalog, scoped by `item_libraries`. `title` is the one kind
    /// the grid shows and the membership pass walks.
    Catalog { title: CatalogKind },
    /// Served from Jellyfin when browsed, never cached.
    Live,
}

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
}

/// `Unknown`: the server's library list was never read into this cache.
/// `Known(vec![])`: the server says the user sees no library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryRegistry {
    Unknown,
    Known(Vec<MediaLibrary>),
}
