//! Library grids: the one place that decides which tier answers a library
//! route. Only the catalog tier is served so far; a live library answers
//! [`BrowseError::LiveLibrary`].

use serde::Serialize;

use super::libraries::{BrowseCapabilities, FilterKey, LibraryId, LibraryRegistry, MediaLibrary};
use super::{ItemPage, ItemQuery, ItemSort, ItemSummary, KindFilter, Library, TitleKind};

#[derive(Debug)]
pub enum BrowseError {
    /// The server's library list has not been read into this cache yet.
    NotLoaded,
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

/// The genre and parental rating choices of one library's titles.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions {
    pub genres: Vec<String>,
    pub official_ratings: Vec<String>,
}

/// One page of a library's grid. `query`'s kinds, search and library are the
/// library's own, whatever the caller set.
pub fn library_page(
    library: &Library,
    id: &LibraryId,
    query: &ItemQuery,
) -> Result<ItemPage, BrowseError> {
    let entry = registered(library, id)?;
    let query = catalog_query(&entry, query)?;
    Ok(library.query(&query)?)
}

pub fn library_letters(
    library: &Library,
    id: &LibraryId,
    query: &ItemQuery,
) -> Result<Vec<(String, i64)>, BrowseError> {
    let entry = registered(library, id)?;
    if !entry.kind().profile().browse.letter_index {
        return Err(BrowseError::Unsupported(
            "this library has no letter index".to_string(),
        ));
    }
    let query = catalog_query(&entry, query)?;
    Ok(library.letter_index(&query)?)
}

pub fn library_options(library: &Library, id: &LibraryId) -> Result<FilterOptions, BrowseError> {
    let title = catalog_title(&registered(library, id)?)?;
    Ok(FilterOptions {
        genres: library.genres(Some(id))?,
        official_ratings: library.official_ratings(&[title.into()], Some(id))?,
    })
}

/// "Latest in <library>": movies by date added, shows by their newest episode.
pub fn latest(
    library: &Library,
    id: &LibraryId,
    limit: i64,
) -> Result<Vec<ItemSummary>, BrowseError> {
    let title = catalog_title(&registered(library, id)?)?;
    let sort = match title {
        TitleKind::Movie => ItemSort::DateAdded,
        TitleKind::Series => ItemSort::EpisodeAdded,
    };
    Ok(library.query_page(&ItemQuery {
        kinds: KindFilter::one(title),
        library: Some(id.clone()),
        sort,
        limit,
        ..ItemQuery::default()
    })?)
}

fn registered(library: &Library, id: &LibraryId) -> Result<MediaLibrary, BrowseError> {
    let LibraryRegistry::Known(libraries) = library.libraries()? else {
        return Err(BrowseError::NotLoaded);
    };
    libraries
        .into_iter()
        .find(|entry| entry.id == *id)
        .ok_or(BrowseError::UnknownLibrary)
}

fn catalog_title(entry: &MediaLibrary) -> Result<TitleKind, BrowseError> {
    entry.catalog_title().ok_or(BrowseError::LiveLibrary)
}

fn catalog_query(entry: &MediaLibrary, query: &ItemQuery) -> Result<ItemQuery, BrowseError> {
    let title = catalog_title(entry)?;
    check(query, entry.kind().profile().browse)?;
    Ok(ItemQuery {
        search: None,
        kinds: KindFilter::one(title),
        library: Some(entry.id.clone()),
        ..query.clone()
    })
}

fn check(query: &ItemQuery, browse: &BrowseCapabilities) -> Result<(), BrowseError> {
    let sort = query.sort.key();
    if !browse.sorts.contains(&sort) {
        return Err(BrowseError::Unsupported(format!(
            "this library cannot sort by {}",
            sort.id()
        )));
    }
    let filters = [
        (query.genre.is_some(), FilterKey::Genre),
        (query.release_decade.is_some(), FilterKey::Decade),
        (query.watched.is_some(), FilterKey::Watched),
        (query.official_rating.is_some(), FilterKey::Parental),
        (query.series_status.is_some(), FilterKey::SeriesStatus),
        (query.favorite.is_some(), FilterKey::Favorite),
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
