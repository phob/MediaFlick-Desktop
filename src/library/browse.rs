//! Library grids: the one place that decides which tier answers a library
//! route. Only the catalog tier is served so far; a live library answers
//! [`BrowseError::LiveLibrary`].

use super::libraries::{BrowseCapabilities, FilterKey, GridSource, LibraryId};
use super::{
    CatalogKind, FilterOptions, ItemPage, ItemQuery, ItemSort, ItemSummary, KindFilter, Library,
    SeriesStatus, SortDirection, WatchFilter,
};

const CATALOG_PAGE_LIMIT: i64 = 500;

/// [`ItemQuery`]'s sort and filters without `search`, `kinds` or `library`:
/// on a library route the library supplies those.
#[derive(Debug, Clone, Default)]
pub struct BrowseQuery {
    pub sort: ItemSort,
    pub direction: Option<SortDirection>,
    pub genre: Option<String>,
    /// Inclusive first year of a standard ten-year release decade.
    pub release_decade: Option<i64>,
    pub watched: Option<WatchFilter>,
    pub official_rating: Option<String>,
    pub series_status: Option<SeriesStatus>,
    pub favorite: Option<bool>,
    pub offset: i64,
    pub limit: i64,
}

impl BrowseQuery {
    fn check(&self, browse: &BrowseCapabilities) -> Result<(), BrowseError> {
        let sort = self.sort.key();
        if !browse.sorts.contains(&sort) {
            return Err(BrowseError::Unsupported(format!(
                "this library cannot sort by {}",
                sort.id()
            )));
        }
        let filters = [
            (self.genre.is_some(), FilterKey::Genre),
            (self.release_decade.is_some(), FilterKey::Decade),
            (self.watched.is_some(), FilterKey::Watched),
            (self.official_rating.is_some(), FilterKey::Parental),
            (self.series_status.is_some(), FilterKey::SeriesStatus),
            (self.favorite.is_some(), FilterKey::Favorite),
        ];
        match filters
            .into_iter()
            .find(|(set, filter)| *set && !browse.filters.contains(filter))
        {
            Some((_, filter)) => Err(BrowseError::Unsupported(format!(
                "this library cannot filter by {}",
                filter.id()
            ))),
            None => Ok(()),
        }
    }

    fn catalog_query(&self, library: &LibraryId, title: CatalogKind) -> ItemQuery {
        ItemQuery {
            search: None,
            kinds: KindFilter::Only(vec![title]),
            library: Some(library.clone()),
            genre: self.genre.clone(),
            release_decade: self.release_decade,
            watched: self.watched,
            favorite: self.favorite,
            official_rating: self.official_rating.clone(),
            series_status: self.series_status,
            sort: self.sort,
            direction: self.direction,
            offset: self.offset.max(0),
            limit: self.limit.clamp(1, CATALOG_PAGE_LIMIT),
        }
    }
}

#[derive(Debug)]
pub enum BrowseError {
    /// Not one of this account's libraries.
    UnknownLibrary,
    /// A sort or filter outside the library's capabilities.
    Unsupported(String),
    /// A library served live from Jellyfin, which this version does not browse.
    LiveLibrary,
    Storage(rusqlite::Error),
}

impl From<rusqlite::Error> for BrowseError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

pub fn library_page(
    library: &Library,
    id: &LibraryId,
    query: &BrowseQuery,
) -> Result<ItemPage, BrowseError> {
    let (title, browse) = catalog_library(library, id)?;
    query.check(browse)?;
    Ok(library.query(&query.catalog_query(id, title))?)
}

pub fn library_letters(
    library: &Library,
    id: &LibraryId,
    query: &BrowseQuery,
) -> Result<Vec<(String, i64)>, BrowseError> {
    let (title, browse) = catalog_library(library, id)?;
    query.check(browse)?;
    Ok(library.letter_index(&query.catalog_query(id, title))?)
}

pub fn library_options(library: &Library, id: &LibraryId) -> Result<FilterOptions, BrowseError> {
    catalog_library(library, id)?;
    Ok(library.library_filter_options(id)?)
}

/// "Latest in <library>": movies by date added, shows by their newest episode.
pub fn latest(
    library: &Library,
    id: &LibraryId,
    limit: i64,
) -> Result<Vec<ItemSummary>, BrowseError> {
    let (title, _) = catalog_library(library, id)?;
    let sort = match title {
        CatalogKind::Series => ItemSort::EpisodeAdded,
        CatalogKind::Movie | CatalogKind::Season | CatalogKind::Episode => ItemSort::DateAdded,
    };
    Ok(library.query_page(&ItemQuery {
        kinds: KindFilter::Only(vec![title]),
        library: Some(id.clone()),
        sort,
        limit,
        ..ItemQuery::default()
    })?)
}

fn catalog_library(
    library: &Library,
    id: &LibraryId,
) -> Result<(CatalogKind, &'static BrowseCapabilities), BrowseError> {
    let registry = library.libraries()?;
    let profile = registry
        .get(id)
        .ok_or(BrowseError::UnknownLibrary)?
        .kind()
        .profile();
    match profile.grid {
        GridSource::Catalog { title } => Ok((title, profile.browse)),
        GridSource::Live => Err(BrowseError::LiveLibrary),
    }
}
