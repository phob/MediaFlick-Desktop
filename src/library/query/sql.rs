use std::cmp::Ordering;

use rusqlite::types::Value as SqlValue;

use super::Facet;
use crate::library::{CatalogKind, ItemQuery, ItemSort, SortDirection, WatchFilter};

const SERIES_HAS_STARTED_EPISODE: &str = "COALESCE(sa.started_episode_count, 0) > 0";

const SERIES_HAS_UNWATCHED_PRESENT_EPISODE: &str = "COALESCE(sa.unplayed_present_count, 0) > 0";

const MOVIE_IN_PROGRESS: &str =
    "COALESCE(u.playback_position_ticks, 0) > 0 AND COALESCE(u.played, 0) = 0";

fn in_progress() -> String {
    format!(
        "CASE WHEN i.kind = 'Series' THEN {SERIES_HAS_STARTED_EPISODE} \
        AND {SERIES_HAS_UNWATCHED_PRESENT_EPISODE} ELSE {MOVIE_IN_PROGRESS} END"
    )
}

pub(in crate::library) struct ItemSql {
    pub(super) from_clause: String,
    pub(super) where_clause: String,
    pub(super) order: String,
    pub(super) has_search: bool,
    pub(super) arguments: Vec<SqlValue>,
}

#[derive(Clone, Copy)]
pub(in crate::library) struct QueryPlan {
    pub(super) user_data: bool,
    pub(super) series_aggregates: bool,
    pub(super) drive_by_library: bool,
    pub(super) letter_cover: bool,
}

pub(super) fn query_base(query: &ItemQuery, plan: QueryPlan) -> ItemSql {
    let mut conditions = Vec::new();
    let mut arguments = Vec::new();

    let kinds = query.kinds.kinds();
    let top_level_only = kinds
        .iter()
        .all(|kind| matches!(kind, CatalogKind::Movie | CatalogKind::Series));
    let fts_table = if top_level_only {
        "catalog_search_fts"
    } else {
        "items_fts"
    };

    let search = query.search.as_deref().and_then(fts_match_expression);
    let has_search = search.is_some();
    let join_library = !has_search && plan.drive_by_library && query.library.is_some();
    let use_browse_cover = !has_search
        && !join_library
        && !plan.letter_cover
        && matches!(query.sort, ItemSort::Name)
        && matches!(
            query
                .direction
                .unwrap_or_else(|| query.sort.natural_direction()),
            SortDirection::Ascending
        )
        && top_level_only
        && kinds.len() == 1
        && query.favorite.is_none()
        && query.watched.is_none()
        && query.genre.is_none()
        && query.studio.is_none()
        && query.tag.is_none()
        && query.official_rating.is_none()
        && query.release_decade.is_none()
        && query.series_status.is_none();
    let (mut from_clause, order) = if has_search {
        (
            format!("FROM {fts_table} JOIN items i ON i.id = {fts_table}.rowid"),
            format!("bm25({fts_table}), i.id ASC"),
        )
    } else if join_library {
        (
            "FROM item_libraries l JOIN items i ON i.jellyfin_id = l.item_id".to_string(),
            query.sort.order_clause(
                query
                    .direction
                    .unwrap_or_else(|| query.sort.natural_direction()),
            ),
        )
    } else {
        let indexed = if plan.letter_cover {
            "FROM items i INDEXED BY items_top_letter_cover"
        } else if use_browse_cover {
            "FROM items i INDEXED BY items_browse_cover"
        } else {
            "FROM items i"
        };
        (
            indexed.to_string(),
            query.sort.order_clause(
                query
                    .direction
                    .unwrap_or_else(|| query.sort.natural_direction()),
            ),
        )
    };
    let inner_user_join =
        query.favorite == Some(true) || matches!(query.watched, Some(WatchFilter::Watched));
    if inner_user_join {
        from_clause.push_str(" JOIN user_data u ON u.jellyfin_id = i.jellyfin_id");
    } else if plan.user_data {
        from_clause.push_str(" LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id");
    }
    if plan.series_aggregates {
        from_clause.push_str(" LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id");
    }
    if let Some(search) = search {
        conditions.push(format!("{fts_table} MATCH ?"));
        arguments.push(SqlValue::Text(search));
    } else if query.search.is_some() {
        conditions.push("0".to_string());
    }

    push_item_filters(query, &kinds, join_library, &mut conditions, &mut arguments);

    let where_clause = format!(" WHERE {}", conditions.join(" AND "));
    ItemSql {
        from_clause,
        where_clause,
        order,
        has_search,
        arguments,
    }
}

fn push_item_filters(
    query: &ItemQuery,
    kinds: &[CatalogKind],
    join_library: bool,
    conditions: &mut Vec<String>,
    arguments: &mut Vec<SqlValue>,
) {
    if kinds.len() == 1 {
        if matches!(kinds[0], CatalogKind::Movie | CatalogKind::Series) {
            conditions.push("i.kind IN ('Movie', 'Series')".to_string());
        }
        conditions.push(format!("i.kind = '{}'", kinds[0].as_str()));
    } else {
        let kind_literals = kinds
            .iter()
            .map(|kind| format!("'{}'", kind.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        conditions.push(format!("i.kind IN ({kind_literals})"));
    }
    if let Some(library) = &query.library {
        if join_library {
            conditions.push("l.library_id = ?".to_string());
        } else {
            conditions.push(
                "EXISTS (SELECT 1 FROM item_libraries l
                     WHERE l.library_id = ? AND l.item_id = i.jellyfin_id)"
                    .to_string(),
            );
        }
        arguments.push(SqlValue::Text(library.as_str().to_string()));
    }
    for (facet, value) in [
        (Facet::Genre, &query.genre),
        (Facet::Studio, &query.studio),
        (Facet::Tag, &query.tag),
    ] {
        if let Some(value) = value {
            let (table, column) = facet.table();
            conditions.push(format!(
                "i.id IN (SELECT item_id FROM {table} WHERE {column} = ?)"
            ));
            arguments.push(SqlValue::Text(value.clone()));
        }
    }
    if let Some(decade) = query.release_decade {
        conditions.push("i.year >= ? AND i.year < ?".to_string());
        arguments.push(SqlValue::Integer(decade));
        arguments.push(SqlValue::Integer(decade.saturating_add(10)));
    }
    if let Some(watched) = query.watched {
        conditions.push(match watched {
            WatchFilter::Unwatched => "COALESCE(u.played, 0) = 0".to_string(),
            WatchFilter::Watched => "u.played = 1".to_string(),
            WatchFilter::InProgress => in_progress(),
        });
    }
    if let Some(rating) = &query.official_rating {
        conditions.push("i.official_rating = ?".to_string());
        arguments.push(SqlValue::Text(rating.clone()));
    }
    if let Some(status) = query.series_status {
        conditions.push("i.series_status = ?".to_string());
        arguments.push(SqlValue::Text(status.as_str().to_string()));
    }
    if let Some(favorite) = query.favorite {
        conditions.push(if favorite {
            "u.is_favorite = 1".to_string()
        } else {
            "COALESCE(u.is_favorite, 0) = 0".to_string()
        });
    }
}

pub(super) fn natural_order(left: &str, right: &str) -> Ordering {
    fn chunks(text: &str) -> impl Iterator<Item = (bool, String)> + '_ {
        let mut rest = text;
        std::iter::from_fn(move || {
            let first = rest.chars().next()?;
            let digits = first.is_ascii_digit();
            let end = rest
                .find(|ch: char| ch.is_ascii_digit() != digits)
                .unwrap_or(rest.len());
            let (chunk, tail) = rest.split_at(end);
            rest = tail;
            Some((digits, chunk.to_lowercase()))
        })
    }
    let compare = |(left_digits, left): (bool, String), (right_digits, right): (bool, String)| {
        if left_digits && right_digits {
            let left = left.trim_start_matches('0');
            let right = right.trim_start_matches('0');
            left.len().cmp(&right.len()).then_with(|| left.cmp(right))
        } else {
            left.cmp(&right)
        }
    };
    let mut left_chunks = chunks(left);
    let mut right_chunks = chunks(right);
    loop {
        match (left_chunks.next(), right_chunks.next()) {
            (None, None) => return left.cmp(right),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_chunk), Some(right_chunk)) => {
                let order = compare(left_chunk, right_chunk);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

pub(super) fn fts_match_expression(input: &str) -> Option<String> {
    let tokens = input
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\"*"))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" AND "))
}
