use serde::Serialize;

use crate::jellyfin::api::model::{BaseItemDto, ItemsResponse};

use super::libraries::{BrowseCapabilities, FilterKey, LibraryId, LibraryRegistry, MediaLibrary};
use super::model::is_synced_kind;
use super::{
    CatalogKind, DEFAULT_PAGE_SIZE, GenericItemDetail, ItemDetail, ItemPage, ItemQuery, ItemSort,
    ItemSummary, KindFilter, Library, MAX_CHILD_PAGE_LIMIT, TitleDetail, TitleKind,
};

#[derive(Debug)]
pub enum BrowseError {
    RegistryUnknown,
    UnknownLibrary,
    Unsupported(String),
    LiveLibrary,
    Storage(rusqlite::Error),
}

impl From<rusqlite::Error> for BrowseError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions {
    pub genres: Vec<String>,
    pub official_ratings: Vec<String>,
}

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

/// The title page of a cached movie, series, season or episode, or `None` when
/// the cache does not hold the item.
pub fn cached_title(library: &Library, item_id: &str) -> rusqlite::Result<Option<TitleDetail>> {
    let Some(item) = library.item(item_id)? else {
        return Ok(None);
    };
    let library_id = library.library_of(item_id)?;
    Ok(Some(TitleDetail { item, library_id }))
}

#[derive(Debug)]
pub enum DetailError {
    /// The signed-in account changed between the fetch and the cache write.
    AccountChanged,
    Storage(rusqlite::Error),
}

impl From<rusqlite::Error> for DetailError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

/// The details page of an item fetched from the server.
pub enum LiveDetail<'a> {
    /// Anything that is not a movie, series, season or episode.
    Generic(Box<GenericItemDetail>),
    /// A title, which is cached before its page is served.
    Title(LiveTitle<'a>),
}

/// A fetched movie, series, season or episode.
#[derive(Clone, Copy)]
pub struct LiveTitle<'a>(&'a BaseItemDto);

pub fn live_detail(dto: &BaseItemDto) -> LiveDetail<'_> {
    if is_synced_kind(dto.item_type.as_deref().unwrap_or_default()) {
        LiveDetail::Title(LiveTitle(dto))
    } else {
        LiveDetail::Generic(Box::new(GenericItemDetail::from_dto(dto)))
    }
}

/// Caches a fetched title the cache did not hold and reads its title page
/// back. The caller runs it under the check that the account which fetched
/// the item is still signed in.
pub fn write_live_title(
    library: &Library,
    title: LiveTitle<'_>,
) -> Result<TitleDetail, DetailError> {
    let LiveTitle(dto) = title;
    library.ingest_page(std::slice::from_ref(dto))?;
    Ok(
        cached_title(library, &dto.id)?.unwrap_or_else(|| TitleDetail {
            item: ItemDetail::from_dto(dto),
            library_id: None,
        }),
    )
}

/// Where `/api/item/{id}/children` reads from.
pub enum ChildrenSource {
    /// A cached title's children, from the cache.
    Catalog(CatalogKind),
    /// Any other container's children, from the server.
    Live,
}

pub fn children_source(library: &Library, item_id: &str) -> rusqlite::Result<ChildrenSource> {
    Ok(library
        .catalog_kind(item_id)?
        .map_or(ChildrenSource::Live, ChildrenSource::Catalog))
}

/// One page of a container's children, as the children route bounds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildPage {
    offset: i64,
    limit: i64,
}

impl ChildPage {
    /// `None` when the request names neither: a cached series or season then
    /// answers with every child, as the season browser asks. A limit outside
    /// `1..=200` is clamped.
    pub fn new(offset: Option<i64>, limit: Option<i64>) -> Result<Option<Self>, &'static str> {
        if offset.is_some_and(|offset| offset < 0) {
            return Err("offset must not be negative");
        }
        if offset.is_none() && limit.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            offset: offset.unwrap_or(0),
            limit: limit
                .unwrap_or(DEFAULT_PAGE_SIZE)
                .clamp(1, MAX_CHILD_PAGE_LIMIT),
        }))
    }

    pub fn offset(self) -> i64 {
        self.offset
    }

    pub fn limit(self) -> i64 {
        self.limit
    }
}

impl Default for ChildPage {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_PAGE_SIZE,
        }
    }
}

/// A cached title's children, cut to `page` when one was asked for.
pub fn catalog_children_page(children: Vec<ItemSummary>, page: Option<ChildPage>) -> ItemPage {
    let total = i64::try_from(children.len()).unwrap_or(i64::MAX);
    let items = match page {
        Some(page) => children
            .into_iter()
            .skip(usize::try_from(page.offset).unwrap_or(usize::MAX))
            .take(usize::try_from(page.limit).unwrap_or(0))
            .collect(),
        None => children,
    };
    ItemPage { items, total }
}

/// A live container's page of children as the server sent it.
pub fn live_children_page(response: &ItemsResponse) -> ItemPage {
    ItemPage {
        items: response.items.iter().map(ItemSummary::from_dto).collect(),
        total: response.total_record_count,
    }
}

fn registered(library: &Library, id: &LibraryId) -> Result<MediaLibrary, BrowseError> {
    let LibraryRegistry::Known(libraries) = library.libraries()? else {
        return Err(BrowseError::RegistryUnknown);
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
