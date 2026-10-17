use super::home_settings::{ResolvedHome, resolved_home, settings_view};
use super::*;

pub(super) fn home(services: &Arc<Services>) -> Handled {
    let home = resolved_home(services)?;
    let watching_enabled = element_enabled(&home.settings, HomeBuiltIn::Watching);
    let continue_watching = if watching_enabled && home.settings.watching.continue_watching {
        services
            .library
            .continue_watching(HOME_ROW_LIMIT)
            .unwrap_or_default()
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
            HomeElementId::BuiltIn { id } => built_in_home_row(services, &home.account, *id),
            HomeElementId::Genre { id } if home.genres.contains(id) => Some(home_row(
                "genre",
                id,
                id,
                &home_query(
                    &services.library,
                    ItemQuery {
                        genre: Some(id.clone()),
                        sort: ItemSort::CommunityRating,
                        ..Default::default()
                    },
                ),
            )),
            HomeElementId::Collection { id } if home.mediaflick_collections => {
                home_collection(services, &home, id)
            }
            _ => None,
        };
        if let Some(row) = row
            && row["items"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        {
            rows.push(row);
        }
    }
    Ok(ApiResponse::untyped(json!({
        "configuration": settings_view(&home, &home.settings),
        "continueWatching": continue_watching,
        "rows": rows,
    })))
}

fn built_in_home_row(services: &Services, account: &AccountKey, id: HomeBuiltIn) -> Option<Value> {
    let (row_id, title, items) = match id {
        HomeBuiltIn::Watching | HomeBuiltIn::Upcoming => return None,
        HomeBuiltIn::BecauseYouWatched => return because_you_watched(services, account),
        HomeBuiltIn::RecentlyAdded => (
            "recentlyAdded",
            "Recently Added Movies",
            home_query(
                &services.library,
                ItemQuery {
                    kinds: KindFilter::one(CatalogKind::Movie),
                    sort: ItemSort::DateAdded,
                    ..Default::default()
                },
            ),
        ),
        HomeBuiltIn::RecentlyAddedShows => (
            "recentlyAddedShows",
            "Recently Added Shows",
            services
                .library
                .recently_added_episodes(HOME_ROW_LIMIT as usize)
                .unwrap_or_default(),
        ),
        HomeBuiltIn::LatestMovies => (
            "latestMovies",
            "Latest Movies",
            latest_home_items(&services.library, CatalogKind::Movie),
        ),
        HomeBuiltIn::LatestShows => (
            "latestShows",
            "Latest Shows",
            latest_home_items(&services.library, CatalogKind::Series),
        ),
        HomeBuiltIn::MyList => (
            "myList",
            "Favorites",
            home_query(
                &services.library,
                ItemQuery {
                    favorite: Some(true),
                    sort: ItemSort::DateAdded,
                    ..Default::default()
                },
            ),
        ),
    };
    Some(home_row("builtIn", row_id, title, &items))
}

fn element_enabled(settings: &HomeSettings, id: HomeBuiltIn) -> bool {
    settings
        .elements
        .iter()
        .any(|element| element.enabled && element.element == HomeElementId::BuiltIn { id })
}

fn home_query(library: &Library, mut query: ItemQuery) -> Vec<ItemSummary> {
    query.limit = HOME_ROW_LIMIT;
    library.query_page(&query).unwrap_or_default()
}

fn home_row(kind: &str, id: &str, title: &str, items: &[ItemSummary]) -> Value {
    json!({ "kind": kind, "id": id, "title": title, "items": items })
}

fn because_you_watched(services: &Services, account: &AccountKey) -> Option<Value> {
    let seed = {
        let mut seeds = services
            .home_watched
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        seeds
            .entry(account.clone())
            .or_insert_with(|| {
                services
                    .library
                    .random_played_movie_with_genre()
                    .unwrap_or_default()
            })
            .clone()
    }?;
    let genre = seed.genres.first()?;
    let items = home_query(
        &services.library,
        ItemQuery {
            genre: Some(genre.to_string()),
            watched: Some(WatchFilter::Unwatched),
            sort: ItemSort::CommunityRating,
            ..Default::default()
        },
    );
    (!items.is_empty()).then(|| {
        home_row(
            "builtIn",
            "becauseYouWatched",
            &format!("Because you watched {}", seed.summary.name),
            &items,
        )
    })
}

fn home_collection(services: &Services, home: &ResolvedHome, profile_id: &str) -> Option<Value> {
    let profile = home
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id && profile.available_on_home)?;
    let snapshot = crate::collections::snapshots::SnapshotRepository::new(&services.library)
        .profile(&home.account, &profile.id, &profile.revision)
        .ok()??;
    let classified = crate::collections::matching::classify(
        &services.library,
        &home.account,
        &snapshot.items,
        crate::collections::matching::OwnershipPolicy {
            complete_sync: crate::library::sync::ownership_available(&services.library),
            restricted_user: services.session.user_restricted(),
        },
    )
    .ok()?;
    let ids = classified
        .owned
        .iter()
        .filter_map(|title| title.local_items.first())
        .map(|item| item.id.clone())
        .take(HOME_ROW_LIMIT as usize)
        .collect::<Vec<_>>();
    let by_id = services
        .library
        .items_by_ids(&ids)
        .ok()?
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect::<HashMap<_, _>>();
    let items = ids
        .iter()
        .filter_map(|id| by_id.get(id).cloned())
        .collect::<Vec<_>>();
    Some(home_row("collection", &profile.id, &profile.title, &items))
}

pub(super) fn home_resume(services: &Arc<Services>) -> Handled {
    let home = resolved_home(services)?;
    if !element_enabled(&home.settings, HomeBuiltIn::Watching) {
        return Ok(ApiResponse::untyped(
            json!({ "continueWatching": [], "nextUp": [] }),
        ));
    }
    let resume = if home.settings.watching.continue_watching {
        services
            .library
            .continue_watching(HOME_ROW_LIMIT)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let next_up = if home.settings.watching.next_up {
        let fetched = services.session.scope().and_then(|scope| {
            items::fetch_next_up(scope.client(), scope.user_id(), None, HOME_ROW_LIMIT)
                .inspect_err(|error| services.session.note_scoped_error(&scope, error))
        });
        fetched
            .map(|response| {
                response
                    .items
                    .iter()
                    .map(ItemSummary::from_dto)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|error| {
                tracing::debug!(target: "app.api", "Next Up unavailable: {error}");
                Vec::new()
            })
    } else {
        Vec::new()
    };
    Ok(ApiResponse::untyped(json!({
        "continueWatching": resume,
        "nextUp": deduplicate_next_up(&resume, next_up),
    })))
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

fn latest_home_items(library: &Library, kind: CatalogKind) -> Vec<ItemSummary> {
    home_query(
        library,
        ItemQuery {
            kinds: KindFilter::one(kind),
            sort: ItemSort::ReleaseDate,
            ..Default::default()
        },
    )
}

pub(super) fn billboard(services: &Arc<Services>) -> Handled {
    if !resolved_home(services)?.settings.billboard {
        return Ok(ApiResponse::untyped(json!({ "items": [] })));
    }
    match services.library.random_billboard_titles(BILLBOARD_LIMIT) {
        Ok(items) => Ok(ApiResponse::untyped(json!({ "items": items }))),
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
        library.upsert_page(&items).expect("seed");

        let movies = latest_home_items(&library, CatalogKind::Movie);
        let shows = latest_home_items(&library, CatalogKind::Series);

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
}
