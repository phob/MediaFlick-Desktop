use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params_from_iter};

use super::{ItemSql, SUMMARY_COLUMNS, fts_match_expression, summary_row, summary_row_at};
use crate::library::{CatalogKind, ItemPage, ItemQuery, ItemSort, ItemSummary, SortDirection};

pub(super) fn uses_single_pass_query(query: &ItemQuery) -> bool {
    query
        .search
        .as_deref()
        .and_then(fts_match_expression)
        .is_some()
        || matches!(
            query.sort,
            ItemSort::Random { .. } | ItemSort::EpisodeAdded | ItemSort::DatePlayed
        )
}

pub(super) fn query_and_count_single_pass(
    connection: &Connection,
    query: &ItemQuery,
) -> rusqlite::Result<ItemPage> {
    let limit = query.limit.clamp(1, 500);
    let offset = query.offset.max(0);
    if let Some(search) = query.search.as_deref().and_then(fts_match_expression)
        && let Some(result) =
            try_unfiltered_fts_page_with_count(connection, query, &search, limit, offset)?
    {
        return Ok(result);
    }
    let (
        plan,
        ItemSql {
            from_clause,
            where_clause,
            order,
            has_search,
            arguments,
        },
    ) = super::paged_query_base(query, limit, offset);
    if has_search {
        let sql = format!(
            "WITH matched AS MATERIALIZED (
                 SELECT i.id AS page_id, bm25({}) AS score
                 {from_clause}{where_clause}
             ),
             counted AS (SELECT count(*) AS total FROM matched),
             page AS (
                 SELECT page_id, score FROM matched
                 ORDER BY score ASC, page_id ASC
                 LIMIT ? OFFSET ?
             )
             SELECT (SELECT total FROM counted), {SUMMARY_COLUMNS}
             FROM page
             JOIN items i ON i.id = page.page_id
             LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
             LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
             ORDER BY page.score ASC, page.page_id ASC",
            fts_table_for(query)
        );
        return read_counted_page(connection, &sql, &arguments);
    }
    if let ItemSort::Random { seed } = query.sort
        && !plan.user_data
        && !plan.series_aggregates
    {
        let sql = format!(
            "WITH matched AS MATERIALIZED (
                 SELECT i.id AS page_id
                 {from_clause}{where_clause}
             ),
             counted AS (SELECT count(*) AS total FROM matched),
             page AS (
                 SELECT m.page_id
                 FROM matched m
                 ORDER BY mediaflick_shuffle(m.page_id, {seed}), m.page_id ASC
                 LIMIT ? OFFSET ?
             )
             SELECT (SELECT total FROM counted), {SUMMARY_COLUMNS}
             FROM page
             JOIN items i ON i.id = page.page_id
             LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
             LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
             ORDER BY mediaflick_shuffle(i.id, {seed}), i.id ASC"
        );
        return read_counted_page(connection, &sql, &arguments);
    }
    let sql = format!(
        "WITH matched AS MATERIALIZED (
             SELECT i.id AS page_id
             {from_clause}{where_clause}
         ),
         counted AS (SELECT count(*) AS total FROM matched),
         page AS (
             SELECT i.id AS page_id
             FROM matched m
             JOIN items i ON i.id = m.page_id
             {}
             {}
             ORDER BY {order}
             LIMIT ? OFFSET ?
         )
         SELECT (SELECT total FROM counted), {SUMMARY_COLUMNS}
         FROM page
         JOIN items i ON i.id = page.page_id
         LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
         LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
         ORDER BY {order}",
        if plan.user_data {
            "LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id"
        } else {
            ""
        },
        if plan.series_aggregates {
            "LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id"
        } else {
            ""
        },
    );
    read_counted_page(connection, &sql, &arguments)
}

fn try_unfiltered_fts_page_with_count(
    connection: &Connection,
    query: &ItemQuery,
    search: &str,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Option<ItemPage>> {
    if !is_unfiltered_top_level_all(query) {
        return Ok(None);
    }
    let sql = format!(
        "WITH hits AS MATERIALIZED (
             SELECT rowid AS page_id, bm25(catalog_search_fts) AS score
             FROM catalog_search_fts
             WHERE catalog_search_fts MATCH ?1
         ),
         counted AS (SELECT count(*) AS total FROM hits),
         page AS (
             SELECT page_id, score FROM hits
             ORDER BY score ASC, page_id ASC
             LIMIT ?2 OFFSET ?3
         )
         SELECT (SELECT total FROM counted), {SUMMARY_COLUMNS}
         FROM page
         JOIN items i ON i.id = page.page_id
         LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
         LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
         ORDER BY page.score ASC, page.page_id ASC"
    );
    let args = [
        SqlValue::Text(search.to_string()),
        SqlValue::Integer(limit),
        SqlValue::Integer(offset),
    ];
    read_counted_page(connection, &sql, &args).map(Some)
}

fn read_counted_page(
    connection: &Connection,
    sql: &str,
    arguments: &[SqlValue],
) -> rusqlite::Result<ItemPage> {
    let mut statement = connection.prepare_cached(sql)?;
    let mut rows = statement.query(params_from_iter(arguments.iter()))?;
    let mut total = 0_i64;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        total = row.get(0)?;
        items.push(summary_row_at(row, 1)?);
    }
    Ok(ItemPage { items, total })
}

pub(super) fn is_unfiltered_top_level_all(query: &ItemQuery) -> bool {
    let kinds = query.kinds.kinds();
    let has_both = kinds.contains(&CatalogKind::Movie)
        && kinds.contains(&CatalogKind::Series)
        && kinds.len() == 2;
    has_both
        && query.library.is_none()
        && query.genre.is_none()
        && query.studio.is_none()
        && query.tag.is_none()
        && query.release_decade.is_none()
        && query.watched.is_none()
        && query.official_rating.is_none()
        && query.series_status.is_none()
        && query.favorite.is_none()
}

pub(super) fn fts_table_for(query: &ItemQuery) -> &'static str {
    let kinds = query.kinds.kinds();
    if kinds
        .iter()
        .all(|kind| matches!(kind, CatalogKind::Movie | CatalogKind::Series))
    {
        "catalog_search_fts"
    } else {
        "items_fts"
    }
}

pub(super) fn try_play_count_desc_fast(
    connection: &Connection,
    query: &ItemQuery,
) -> rusqlite::Result<Option<Vec<ItemSummary>>> {
    let descending = matches!(
        query
            .direction
            .unwrap_or_else(|| query.sort.natural_direction()),
        SortDirection::Descending
    );
    if !matches!(query.sort, ItemSort::PlayCount)
        || !descending
        || query.search.is_some()
        || query.genre.is_some()
        || query.studio.is_some()
        || query.tag.is_some()
        || query.release_decade.is_some()
        || query.watched.is_some()
        || query.official_rating.is_some()
        || query.series_status.is_some()
        || query.favorite.is_some()
    {
        return Ok(None);
    }
    let limit = query.limit.clamp(1, 500);
    let offset = query.offset.max(0);
    let end = offset.saturating_add(limit);
    if end <= 0 || end > 2000 {
        return Ok(None);
    }
    let kinds = query.kinds.kinds();
    let kind_literals = kinds
        .iter()
        .map(|kind| format!("'{}'", kind.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let lib_join = if query.library.is_some() {
        "JOIN item_libraries l ON l.item_id = i.jellyfin_id AND l.library_id = ?1"
    } else {
        ""
    };
    let Some(cutoff) = fetch_play_count_cutoff(connection, query, &kind_literals, lib_join, end)?
    else {
        return Ok(None);
    };
    fetch_play_count_page(
        connection,
        query,
        &kind_literals,
        lib_join,
        cutoff,
        limit,
        offset,
    )
    .map(Some)
}

fn fetch_play_count_cutoff(
    connection: &Connection,
    query: &ItemQuery,
    kind_literals: &str,
    lib_join: &str,
    end: i64,
) -> rusqlite::Result<Option<i64>> {
    let mut cutoff_args = Vec::new();
    if let Some(library) = &query.library {
        cutoff_args.push(SqlValue::Text(library.as_str().to_string()));
    }
    cutoff_args.push(SqlValue::Integer(end - 1));
    let cutoff_param = if query.library.is_some() { "?2" } else { "?1" };
    let cutoff_sql = format!(
        "SELECT u.play_count
         FROM user_data u INDEXED BY user_data_play_count_desc
         JOIN items i ON i.jellyfin_id = u.jellyfin_id
         {lib_join}
         WHERE u.play_count > 0 AND i.kind IN ({kind_literals})
         ORDER BY u.play_count DESC
         LIMIT 1 OFFSET {cutoff_param}"
    );
    let mut stmt = connection.prepare_cached(&cutoff_sql)?;
    let mut rows = stmt.query(params_from_iter(cutoff_args.iter()))?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get::<_, i64>(0)?).filter(|count| *count > 0)),
        None => Ok(None),
    }
}

fn fetch_play_count_page(
    connection: &Connection,
    query: &ItemQuery,
    kind_literals: &str,
    lib_join: &str,
    cutoff: i64,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<ItemSummary>> {
    let mut page_args = Vec::new();
    if let Some(library) = &query.library {
        page_args.push(SqlValue::Text(library.as_str().to_string()));
    }
    page_args.push(SqlValue::Integer(cutoff));
    page_args.push(SqlValue::Integer(limit));
    page_args.push(SqlValue::Integer(offset));
    let (c_idx, l_idx, o_idx) = if query.library.is_some() {
        ("?2", "?3", "?4")
    } else {
        ("?1", "?2", "?3")
    };
    let page_sql = format!(
        "WITH page AS (
             SELECT i.id AS page_id
             FROM user_data u INDEXED BY user_data_play_count_desc
             JOIN items i ON i.jellyfin_id = u.jellyfin_id
             {lib_join}
             WHERE u.play_count > 0 AND u.play_count >= {c_idx} AND i.kind IN ({kind_literals})
             ORDER BY u.play_count DESC NULLS LAST, i.sort_name COLLATE NOCASE ASC, i.id ASC
             LIMIT {l_idx} OFFSET {o_idx}
         )
         SELECT {SUMMARY_COLUMNS}
         FROM page
         JOIN items i ON i.id = page.page_id
         LEFT JOIN user_data u ON u.jellyfin_id = i.jellyfin_id
         LEFT JOIN series_aggregates sa ON sa.series_id = i.jellyfin_id
         ORDER BY COALESCE(u.play_count, 0) DESC NULLS LAST, i.sort_name COLLATE NOCASE ASC, i.id ASC"
    );
    let mut stmt = connection.prepare_cached(&page_sql)?;
    stmt.query_map(params_from_iter(page_args.iter()), summary_row)?
        .collect()
}
