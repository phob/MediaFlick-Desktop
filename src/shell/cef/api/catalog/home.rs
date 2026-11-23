use std::sync::PoisonError;

use crate::library::ItemDetail;

use super::home_settings::{HomeConfiguration, ResolvedHome, resolved_home, settings_view};
use super::*;

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct HomeResponse {
    configuration: HomeConfiguration,
    continue_watching: Vec<ItemSummary>,
    rows: Vec<HomeRow>,
}

#[derive(Serialize, ts_rs::TS)]
struct HomeRow {
    #[serde(flatten)]
    source: HomeRowSource,
    title: String,
    items: Vec<ItemSummary>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum HomeRowSource {
    BuiltIn { id: HomeBuiltIn },
    Genre { id: String },
    Collection { id: String },
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct HomeResumeResponse {
    continue_watching: Vec<ItemSummary>,
    next_up: Vec<ItemSummary>,
}

#[derive(Serialize, ts_rs::TS)]
pub(in super::super) struct Billboard {
    items: Vec<ItemSummary>,
}

pub(in super::super) fn home_view(services: &Services) -> Result<HomeResponse, ApiResponse> {
    let home = resolved_home(services)?;
    home_rows(services, &home).map_err(|error| storage_failure(&error))
}

fn home_rows(services: &Services, home: &ResolvedHome) -> rusqlite::Result<HomeResponse> {
    let watching_enabled = element_enabled(&home.settings, HomeBuiltIn::Watching);
    let continue_watching = if watching_enabled && home.settings.watching.continue_watching {
        services.library.continue_watching(HOME_ROW_LIMIT)?
    } else {
        Vec::new()
    };
    let mut rows = Vec::new();
    for element in home
        .settings
        .elements
        .iter()
        .filter(|element| element.enabled)
    {
        let row = match &element.element {
            HomeElementId::BuiltIn { id } => built_in_home_row(services, &home.account, *id)?,
            HomeElementId::Genre { id } if home.genres.contains(id) => Some(home_row(
                HomeRowSource::Genre { id: id.clone() },
                id,
                home_query(
                    &services.library,
                    ItemQuery {
                        genre: Some(id.clone()),
                        sort: ItemSort::CommunityRating,
                        ..Default::default()
                    },
                )?,
            )),
            HomeElementId::Collection { id } if home.mediaflick_collections => {
                home_collection(services, home, id)?
            }
            _ => None,
        };
        if let Some(row) = row
            && !row.items.is_empty()
        {
            rows.push(row);
        }
    }
    Ok(HomeResponse {
        configuration: settings_view(home, &home.settings),
        continue_watching,
        rows,
    })
}

fn built_in_home_row(
    services: &Services,
    account: &AccountKey,
    id: HomeBuiltIn,
) -> rusqlite::Result<Option<HomeRow>> {
    let (title, items) = match id {
        HomeBuiltIn::Watching | HomeBuiltIn::Upcoming => return Ok(None),
        HomeBuiltIn::BecauseYouWatched => return because_you_watched(services, account),
        HomeBuiltIn::RecentlyAdded => (
            "Recently Added Movies",
            home_query(
                &services.library,
                ItemQuery {
                    kinds: KindFilter::one(CatalogKind::Movie),
                    sort: ItemSort::DateAdded,
                    ..Default::default()
                },
            )?,
        ),
        HomeBuiltIn::RecentlyAddedShows => (
            "Recently Added Shows",
            services
                .library
                .recently_added_episodes(HOME_ROW_LIMIT as usize)?,
        ),
        HomeBuiltIn::LatestMovies => (
            "Latest Movies",
            latest_home_items(&services.library, CatalogKind::Movie)?,
        ),
        HomeBuiltIn::LatestShows => (
            "Latest Shows",
            latest_home_items(&services.library, CatalogKind::Series)?,
        ),
        HomeBuiltIn::MyList => (
            "Favorites",
            home_query(
                &services.library,
                ItemQuery {
                    favorite: Some(true),
                    sort: ItemSort::DateAdded,
                    ..Default::default()
                },
            )?,
        ),
    };
    Ok(Some(home_row(HomeRowSource::BuiltIn { id }, title, items)))
}

fn element_enabled(settings: &HomeSettings, id: HomeBuiltIn) -> bool {
    settings
        .elements
        .iter()
        .any(|element| element.enabled && element.element == HomeElementId::BuiltIn { id })
}

fn home_query(library: &Library, mut query: ItemQuery) -> rusqlite::Result<Vec<ItemSummary>> {
    query.limit = HOME_ROW_LIMIT;
    library.query_page(&query)
}

fn home_row(source: HomeRowSource, title: &str, items: Vec<ItemSummary>) -> HomeRow {
    HomeRow {
        source,
        title: title.to_string(),
        items,
    }
}

fn because_you_watched(
    services: &Services,
    account: &AccountKey,
) -> rusqlite::Result<Option<HomeRow>> {
    let Some(seed) = watched_seed(services, account)? else {
        return Ok(None);
    };
    let Some(genre) = seed.genres.first() else {
        return Ok(None);
    };
    let items = home_query(
        &services.library,
        ItemQuery {
            genre: Some(genre.to_string()),
            watched: Some(WatchFilter::Unwatched),
            sort: ItemSort::CommunityRating,
            ..Default::default()
        },
    )?;
    Ok((!items.is_empty()).then(|| {
        home_row(
            HomeRowSource::BuiltIn {
                id: HomeBuiltIn::BecauseYouWatched,
            },
            &format!("Because you watched {}", seed.summary.name),
            items,
        )
    }))
}

fn watched_seed(services: &Services, account: &AccountKey) -> rusqlite::Result<Option<ItemDetail>> {
    let kept = services
        .home_watched
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(account)
        .cloned();
    if kept.is_some() {
        return Ok(kept);
    }
    let Some(picked) = services.library.random_played_movie_with_genre()? else {
        return Ok(None);
    };
    Ok(Some(
        services
            .home_watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(account.clone())
            .or_insert(picked)
            .clone(),
    ))
}

fn home_collection(
    services: &Services,
    home: &ResolvedHome,
    profile_id: &str,
) -> rusqlite::Result<Option<HomeRow>> {
    let Some(profile) = home
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id && profile.available_on_home)
    else {
        return Ok(None);
    };
    let Some(snapshot) = crate::collections::snapshots::SnapshotRepository::new(&services.library)
        .profile(&home.account, &profile.id, &profile.revision)?
    else {
        return Ok(None);
    };
    let classified = crate::collections::matching::classify(
        &services.library,
        &home.account,
        &snapshot.items,
        crate::collections::matching::OwnershipPolicy {
            complete_sync: crate::library::sync::ownership_available(&services.library),
            restricted_user: services.session.user_restricted(),
        },
    )?;
    let ids = classified
        .owned
        .iter()
        .filter_map(|title| title.local_items.first())
        .map(|item| item.id.clone())
        .take(HOME_ROW_LIMIT as usize)
        .collect::<Vec<_>>();
    let by_id = services
        .library
        .items_by_ids(&ids)?
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect::<HashMap<_, _>>();
    let items = ids
        .iter()
        .filter_map(|id| by_id.get(id).cloned())
        .collect::<Vec<_>>();
    Ok(Some(home_row(
        HomeRowSource::Collection {
            id: profile.id.clone(),
        },
        &profile.title,
        items,
    )))
}

pub(super) fn home_resume(services: &Arc<Services>) -> Result<HomeResumeResponse, ApiResponse> {
    let home = resolved_home(services)?;
    if !element_enabled(&home.settings, HomeBuiltIn::Watching) {
        return Ok(HomeResumeResponse {
            continue_watching: Vec::new(),
            next_up: Vec::new(),
        });
    }
    let resume = if home.settings.watching.continue_watching {
        services
            .library
            .continue_watching(HOME_ROW_LIMIT)
            .map_err(|error| storage_failure(&error))?
    } else {
        Vec::new()
    };
    let next_up = if home.settings.watching.next_up {
        let fetched = services.session.scope().and_then(|scope| {
            services.session.fetch_current(&scope, |client, user_id| {
                items::fetch_next_up(client, user_id, None, HOME_ROW_LIMIT)
            })
        });
        match fetched {
            Ok(response) => response
                .items
                .iter()
                .map(ItemSummary::from_dto)
                .collect::<Vec<_>>(),
            Err(ApiError::AccountChanged) => return Err(stale_account_response()),
            Err(error) => {
                tracing::debug!(target: "app.api", "Next Up unavailable: {error}");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let next_up = deduplicate_next_up(&resume, next_up);
    Ok(HomeResumeResponse {
        continue_watching: resume,
        next_up,
    })
}

fn deduplicate_next_up(resume: &[ItemSummary], next_up: Vec<ItemSummary>) -> Vec<ItemSummary> {
    fn keys(item: &ItemSummary) -> impl Iterator<Item = &str> {
        [Some(item.id.as_str()), item.series_id.as_deref()]
            .into_iter()
            .flatten()
    }
    let seen = resume.iter().flat_map(keys).collect::<HashSet<_>>();
    next_up
        .into_iter()
        .filter(|item| keys(item).all(|key| !seen.contains(key)))
        .collect()
}

fn latest_home_items(library: &Library, kind: CatalogKind) -> rusqlite::Result<Vec<ItemSummary>> {
    home_query(
        library,
        ItemQuery {
            kinds: KindFilter::one(kind),
            sort: ItemSort::ReleaseDate,
            ..Default::default()
        },
    )
}

pub(in super::super) fn billboard_view(services: &Services) -> Result<Billboard, ApiResponse> {
    if !resolved_home(services)?.settings.billboard {
        return Ok(Billboard { items: Vec::new() });
    }
    match services.library.random_billboard_titles(BILLBOARD_LIMIT) {
        Ok(items) => Ok(Billboard { items }),
        Err(error) => Err(storage_failure(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_home_rows_are_kind_scoped_and_ordered_by_release_year() {
        let library = Library::open_in_memory().expect("library");
        let items: Vec<BaseItemDto> = [
            r#"{"Id":"old-movie","Name":"Old Film","Type":"Movie","ProductionYear":1999}"#,
            r#"{"Id":"new-movie","Name":"New Film","Type":"Movie","ProductionYear":2026}"#,
            r#"{"Id":"old-show","Name":"Old Show","Type":"Series","ProductionYear":2010}"#,
            r#"{"Id":"new-show","Name":"New Show","Type":"Series","ProductionYear":2025}"#,
        ]
        .into_iter()
        .map(|value| serde_json::from_str(value).expect("dto"))
        .collect();
        library
            .upsert_page(&crate::jellyfin::session::Live::for_test(), &items)
            .expect("seed");

        let movies = latest_home_items(&library, CatalogKind::Movie).expect("movies");
        let shows = latest_home_items(&library, CatalogKind::Series).expect("shows");

        assert_eq!(movies[0].id, "new-movie");
        assert_eq!(movies[1].id, "old-movie");
        assert!(movies.iter().all(|item| item.kind == "Movie"));
        assert_eq!(shows[0].id, "new-show");
        assert_eq!(shows[1].id, "old-show");
        assert!(shows.iter().all(|item| item.kind == "Series"));
    }

    fn episode(id: &str, series_id: &str) -> ItemSummary {
        ItemSummary {
            id: id.to_string(),
            kind: "Episode".to_string(),
            series_id: Some(series_id.to_string()),
            ..ItemSummary::default()
        }
    }

    #[test]
    fn a_show_already_being_watched_is_removed_from_next_up() {
        let next_up = deduplicate_next_up(
            &[episode("e1", "sev")],
            vec![
                episode("e1", "sev"),
                episode("e2", "sev"),
                episode("e9", "silo"),
            ],
        );
        let ids = next_up
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["e9"]);
    }

    #[test]
    fn because_you_watched_does_not_cache_missing_seed_before_first_page() {
        let fixture = crate::app::services::test_support::TestServices::signed_in_as(
            "http://127.0.0.1:9",
            "user-byw",
        );
        let account = fixture.services.session.account_key().expect("account");

        assert!(
            because_you_watched(&fixture.services, &account)
                .expect("empty library")
                .is_none()
        );

        let items: Vec<BaseItemDto> = [
            r#"{"Id":"played-1","Name":"Inception","Type":"Movie","Genres":["Sci-Fi"],"UserData":{"Played":true}}"#,
            r#"{"Id":"unwatched-1","Name":"Arrival","Type":"Movie","Genres":["Sci-Fi"],"UserData":{"Played":false}}"#,
        ]
        .into_iter()
        .map(|value| serde_json::from_str(value).expect("dto"))
        .collect();
        fixture
            .services
            .library
            .ingest_page(&crate::jellyfin::session::Live::for_test(), &items)
            .expect("ingest");

        let row = because_you_watched(&fixture.services, &account)
            .expect("populated library")
            .expect("row after catalog items arrive");
        assert_eq!(row.title, "Because you watched Inception");
        assert_eq!(row.items.len(), 1);
        assert_eq!(row.items[0].id, "unwatched-1");

        let second = because_you_watched(&fixture.services, &account)
            .expect("cached seed")
            .expect("row from cached seed");
        assert_eq!(second.title, "Because you watched Inception");
    }
}
