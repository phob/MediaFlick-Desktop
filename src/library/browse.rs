use serde::Serialize;

use crate::jellyfin::api::model::{BaseItemDto, ItemsResponse};

use super::libraries::{
    BrowseCapabilities, FilterKey, GridSource, LibraryId, LibraryRegistry, LiveLatest, LiveShape,
    MediaLibrary,
};
use super::model::is_synced_kind;
use super::{
    CatalogKind, DEFAULT_PAGE_SIZE, GenericItemDetail, ItemDetail, ItemPage, ItemQuery, ItemSort,
    ItemSummary, KindFilter, Library, MAX_CHILD_PAGE_LIMIT, SortDirection, TitleDetail, TitleKind,
};

#[derive(Debug)]
pub enum BrowseError {
    RegistryUnknown,
    UnknownLibrary,
    Unsupported(String),
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

/// A library grid's answer: read from the cache, or a request the caller
/// sends to Jellyfin with its session.
pub enum Grid {
    Cached(ItemPage),
    Live(LiveQuery),
}

pub enum LiveQuery {
    Page(LivePage),
    Latest { parent: LibraryId, limit: i64 },
}

/// One page of a live grid. Every field comes from the library's profile or
/// from a request already checked against its capabilities.
pub struct LivePage {
    pub parent: LibraryId,
    pub shape: LiveShape,
    pub sort: LiveSort,
    pub direction: SortDirection,
    pub favorites_only: bool,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveSort {
    Name,
    DateAdded,
    ReleaseDate,
}

impl LiveSort {
    fn from_sort(sort: ItemSort) -> Option<Self> {
        match sort {
            ItemSort::Name => Some(Self::Name),
            ItemSort::DateAdded => Some(Self::DateAdded),
            ItemSort::ReleaseDate => Some(Self::ReleaseDate),
            ItemSort::EpisodeAdded
            | ItemSort::CommunityRating
            | ItemSort::DatePlayed
            | ItemSort::PlayCount
            | ItemSort::Runtime
            | ItemSort::Random { .. } => None,
        }
    }
}

pub fn library_page(
    library: &Library,
    id: &LibraryId,
    query: &ItemQuery,
) -> Result<Grid, BrowseError> {
    let entry = registered(library, id)?;
    let profile = entry.kind().profile();
    check(query, profile.browse)?;
    match profile.grid {
        GridSource::Catalog { title } => Ok(Grid::Cached(
            library.query(&catalog_query(&entry, title, query))?,
        )),
        GridSource::Live { shape, .. } => Ok(Grid::Live(LiveQuery::Page(checked_live_page(
            entry.id, shape, query,
        )?))),
    }
}

pub fn library_letters(
    library: &Library,
    id: &LibraryId,
    query: &ItemQuery,
) -> Result<Vec<(String, i64)>, BrowseError> {
    let entry = registered(library, id)?;
    let profile = entry.kind().profile();
    let GridSource::Catalog { title } = profile.grid else {
        return Err(no_letter_index());
    };
    if !profile.browse.letter_index {
        return Err(no_letter_index());
    }
    check(query, profile.browse)?;
    Ok(library.letter_index(&catalog_query(&entry, title, query))?)
}

pub fn library_options(library: &Library, id: &LibraryId) -> Result<FilterOptions, BrowseError> {
    let entry = registered(library, id)?;
    match entry.kind().profile().grid {
        GridSource::Catalog { title } => Ok(FilterOptions {
            genres: library.genres(Some(id))?,
            official_ratings: library.official_ratings(&[title.into()], Some(id))?,
        }),
        GridSource::Live { .. } => Ok(FilterOptions {
            genres: Vec::new(),
            official_ratings: Vec::new(),
        }),
    }
}

pub fn latest(library: &Library, id: &LibraryId, limit: i64) -> Result<Grid, BrowseError> {
    let entry = registered(library, id)?;
    match entry.kind().profile().grid {
        GridSource::Catalog { title } => {
            let sort = match title {
                TitleKind::Movie => ItemSort::DateAdded,
                TitleKind::Series => ItemSort::EpisodeAdded,
            };
            let items = library.query_page(&ItemQuery {
                kinds: KindFilter::one(title),
                library: Some(entry.id),
                sort,
                limit,
                ..ItemQuery::default()
            })?;
            let total = i64::try_from(items.len()).unwrap_or(i64::MAX);
            Ok(Grid::Cached(ItemPage { items, total }))
        }
        GridSource::Live {
            latest: LiveLatest::Grouped,
            ..
        } => Ok(Grid::Live(LiveQuery::Latest {
            parent: entry.id,
            limit,
        })),
        GridSource::Live {
            shape,
            latest: LiveLatest::Newest,
        } => Ok(Grid::Live(LiveQuery::Page(LivePage {
            parent: entry.id,
            shape,
            sort: LiveSort::DateAdded,
            direction: SortDirection::Descending,
            favorites_only: false,
            offset: 0,
            limit,
        }))),
    }
}

pub fn cached_title(library: &Library, item_id: &str) -> rusqlite::Result<Option<TitleDetail>> {
    let Some(item) = library.item(item_id)? else {
        return Ok(None);
    };
    let library_id = library.library_of(item_id)?;
    Ok(Some(TitleDetail { item, library_id }))
}

#[derive(Debug)]
pub enum DetailError {
    AccountChanged,
    Storage(rusqlite::Error),
}

impl From<rusqlite::Error> for DetailError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

pub enum LiveDetail<'a> {
    Generic(Box<GenericItemDetail>),
    Title(LiveTitle<'a>),
}

#[derive(Clone, Copy)]
pub struct LiveTitle<'a>(&'a BaseItemDto);

pub fn live_detail(dto: &BaseItemDto) -> LiveDetail<'_> {
    if is_synced_kind(dto.item_type.as_deref().unwrap_or_default()) {
        LiveDetail::Title(LiveTitle(dto))
    } else {
        LiveDetail::Generic(Box::new(GenericItemDetail::from_dto(dto)))
    }
}

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

pub enum ChildrenSource {
    Catalog(CatalogKind),
    Live,
}

pub fn children_source(library: &Library, item_id: &str) -> rusqlite::Result<ChildrenSource> {
    Ok(library
        .catalog_kind(item_id)?
        .map_or(ChildrenSource::Live, ChildrenSource::Catalog))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildPage {
    offset: i64,
    limit: i64,
}

impl ChildPage {
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

pub fn live_page(response: &ItemsResponse) -> ItemPage {
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

fn catalog_query(entry: &MediaLibrary, title: TitleKind, query: &ItemQuery) -> ItemQuery {
    ItemQuery {
        search: None,
        kinds: KindFilter::one(title),
        library: Some(entry.id.clone()),
        ..query.clone()
    }
}

fn checked_live_page(
    parent: LibraryId,
    shape: LiveShape,
    query: &ItemQuery,
) -> Result<LivePage, BrowseError> {
    let sort = LiveSort::from_sort(query.sort).ok_or_else(|| {
        BrowseError::Unsupported(format!(
            "this library cannot sort by {}",
            query.sort.key().id()
        ))
    })?;
    if query.favorite == Some(false) {
        return Err(BrowseError::Unsupported(
            "this library can only filter to favorites".to_string(),
        ));
    }
    if query.offset < 0 {
        return Err(BrowseError::Unsupported(
            "offset must not be negative".to_string(),
        ));
    }
    Ok(LivePage {
        parent,
        shape,
        sort,
        direction: query
            .direction
            .unwrap_or_else(|| query.sort.natural_direction()),
        favorites_only: query.favorite == Some(true),
        offset: query.offset,
        limit: query.limit.clamp(1, MAX_CHILD_PAGE_LIMIT),
    })
}

fn no_letter_index() -> BrowseError {
    BrowseError::Unsupported("this library has no letter index".to_string())
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
