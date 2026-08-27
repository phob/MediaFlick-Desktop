use std::collections::{HashMap, HashSet};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::library::Library;
use crate::preferences::AccountKey;

use super::{
    CanonicalIdentity, ClassifiedSnapshot, ClassifiedTitle, LocalItem, MediaType, NormalizedTitle,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OwnershipPolicy {
    pub complete_sync: bool,
    pub restricted_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryIdentityRequest {
    pub media_type: MediaType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imdb_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tvdb_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedIdentityMapping {
    pub media_type: MediaType,
    pub provider: String,
    pub provider_id: String,
    pub tmdb_id: u64,
}

pub fn unresolved_secondary_identities(
    library: &Library,
) -> rusqlite::Result<Vec<SecondaryIdentityRequest>> {
    library.with_connection(|connection| {
        let mut statement = connection.prepare(
            "SELECT CASE items.kind WHEN 'Movie' THEN 'movie' ELSE 'series' END,
                    'imdb', items.imdb_id
             FROM items
             WHERE items.kind IN ('Movie', 'Series')
               AND items.tmdb_id IS NULL
               AND items.imdb_id IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM provider_identity_map mappings
                   WHERE mappings.media_type = CASE items.kind WHEN 'Movie' THEN 'movie' ELSE 'series' END
                     AND mappings.provider = 'imdb'
                     AND mappings.provider_id = items.imdb_id
               )
             UNION ALL
             SELECT CASE items.kind WHEN 'Movie' THEN 'movie' ELSE 'series' END,
                    'tvdb', items.tvdb_id
             FROM items
             WHERE items.kind IN ('Movie', 'Series')
               AND items.tmdb_id IS NULL
               AND items.tvdb_id IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM provider_identity_map mappings
                   WHERE mappings.media_type = CASE items.kind WHEN 'Movie' THEN 'movie' ELSE 'series' END
                     AND mappings.provider = 'tvdb'
                     AND mappings.provider_id = items.tvdb_id
               )
             LIMIT 500",
        )?;
        statement
            .query_map([], |row| {
                let media_type = match row.get::<_, String>(0)?.as_str() {
                    "movie" => MediaType::Movie,
                    _ => MediaType::Series,
                };
                let provider = row.get::<_, String>(1)?;
                let provider_id = row.get::<_, String>(2)?;
                Ok(SecondaryIdentityRequest {
                    media_type,
                    imdb_id: (provider == "imdb").then_some(provider_id.clone()),
                    tvdb_id: (provider == "tvdb").then_some(provider_id),
                })
            })?
            .collect()
    })
}

pub fn save_identity_mappings(
    library: &Library,
    mappings: &[ResolvedIdentityMapping],
) -> rusqlite::Result<()> {
    library.with_transaction(|transaction| {
        let mut statement = transaction.prepare(
            "INSERT INTO provider_identity_map (
                media_type, provider, provider_id, tmdb_id, resolved_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(media_type, provider, provider_id) DO UPDATE SET
                tmdb_id = excluded.tmdb_id,
                resolved_at = excluded.resolved_at",
        )?;
        let resolved_at = crate::library::now_unix();
        for mapping in mappings.iter().filter(|mapping| {
            matches!(mapping.media_type, MediaType::Movie | MediaType::Series)
                && matches!(mapping.provider.as_str(), "imdb" | "tvdb")
                && !mapping.provider_id.trim().is_empty()
                && mapping.tmdb_id > 0
        }) {
            let Ok(tmdb_id) = i64::try_from(mapping.tmdb_id) else {
                continue;
            };
            statement.execute(params![
                media_type_key(mapping.media_type),
                mapping.provider,
                mapping.provider_id,
                tmdb_id,
                resolved_at,
            ])?;
        }
        Ok(())
    })
}

pub fn owned_tmdb_ids(library: &Library, media_type: MediaType) -> rusqlite::Result<Vec<u64>> {
    let jellyfin_kind = match media_type {
        MediaType::Movie => "Movie",
        MediaType::Series => "Series",
        MediaType::Mixed => return Ok(Vec::new()),
    };
    library.with_connection(|connection| {
        let mut statement = connection.prepare(
            "SELECT DISTINCT tmdb_id FROM (
                SELECT CAST(items.tmdb_id AS TEXT) AS tmdb_id
                FROM items
                WHERE items.kind = ?1 AND items.tmdb_id IS NOT NULL
                UNION
                SELECT CAST(mappings.tmdb_id AS TEXT) AS tmdb_id
                FROM items
                JOIN provider_identity_map mappings
                  ON mappings.media_type = ?2
                 AND ((mappings.provider = 'imdb' AND mappings.provider_id = items.imdb_id)
                      OR (mappings.provider = 'tvdb' AND mappings.provider_id = items.tvdb_id))
                WHERE items.kind = ?1 AND items.tmdb_id IS NULL
             ) ORDER BY CAST(tmdb_id AS INTEGER)",
        )?;
        let values = statement
            .query_map(params![jellyfin_kind, media_type_key(media_type)], |row| {
                row.get::<_, String>(0)
            })?
            .filter_map(Result::ok)
            .filter_map(|value| value.parse::<u64>().ok().filter(|id| *id > 0))
            .collect();
        Ok(values)
    })
}

pub fn classify(
    library: &Library,
    account: &AccountKey,
    provider_items: &[NormalizedTitle],
    policy: OwnershipPolicy,
) -> rusqlite::Result<ClassifiedSnapshot> {
    let items = normalized_provider_items(provider_items);
    if !policy.complete_sync || !active_account_matches(library, account) {
        return Ok(ClassifiedSnapshot {
            items: if policy.restricted_user {
                Vec::new()
            } else {
                items
            },
            ownership_available: false,
            ..ClassifiedSnapshot::default()
        });
    }
    let local = local_item_map(library, &items)?;
    let mut owned = Vec::new();
    let mut missing = Vec::new();
    for item in items {
        match local.get(&item.identity) {
            Some(local_items) if !local_items.is_empty() => owned.push(ClassifiedTitle {
                title: item,
                local_items: local_items.clone(),
            }),
            _ if !policy.restricted_user => missing.push(item),
            _ => {}
        }
    }
    Ok(ClassifiedSnapshot {
        owned,
        missing,
        items: Vec::new(),
        ownership_available: true,
    })
}

fn active_account_matches(library: &Library, account: &AccountKey) -> bool {
    let credentials = library.credentials();
    credentials.server_id.as_deref() == Some(account.server_id())
        && credentials.user_id.as_deref() == Some(account.user_id())
}

fn normalized_provider_items(items: &[NormalizedTitle]) -> Vec<NormalizedTitle> {
    let mut identities = HashSet::new();
    let mut normalized = items
        .iter()
        .filter(|item| !item.adult && identities.insert(item.identity.clone()))
        .cloned()
        .collect::<Vec<_>>();
    normalized.sort_by_key(|item| item.source_order);
    normalized
}

pub(crate) fn local_item_map(
    library: &Library,
    provider_items: &[NormalizedTitle],
) -> rusqlite::Result<HashMap<CanonicalIdentity, Vec<LocalItem>>> {
    library.with_connection(|connection| {
        let mut result: HashMap<CanonicalIdentity, Vec<LocalItem>> = HashMap::new();
        let mut statement = connection.prepare(
            "SELECT DISTINCT items.jellyfin_id, items.name, items.kind,
                    COALESCE(user_data.played, 0)
             FROM items
             LEFT JOIN user_data ON user_data.jellyfin_id = items.jellyfin_id
             LEFT JOIN provider_identity_map imdb_mapping
               ON imdb_mapping.media_type = ?3
              AND imdb_mapping.provider = 'imdb'
              AND imdb_mapping.provider_id = items.imdb_id
             LEFT JOIN provider_identity_map tvdb_mapping
               ON tvdb_mapping.media_type = ?3
              AND tvdb_mapping.provider = 'tvdb'
              AND tvdb_mapping.provider_id = items.tvdb_id
             WHERE items.kind = ?1
               AND (items.tmdb_id = ?2
                    OR imdb_mapping.tmdb_id = ?2
                    OR tvdb_mapping.tmdb_id = ?2)
             ORDER BY items.jellyfin_id",
        )?;
        for item in provider_items {
            let jellyfin_kind = match item.identity.media_type {
                MediaType::Movie => "Movie",
                MediaType::Series => "Series",
                MediaType::Mixed => continue,
            };
            let rows = statement
                .query_map(
                    params![
                        jellyfin_kind,
                        item.identity.tmdb_id.to_string(),
                        media_type_key(item.identity.media_type),
                    ],
                    |row| {
                        Ok(LocalItem {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            kind: row.get(2)?,
                            played: row.get(3)?,
                        })
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if !rows.is_empty() {
                result.insert(item.identity.clone(), rows);
            }
        }
        Ok(result)
    })
}

fn media_type_key(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Movie => "movie",
        MediaType::Series => "series",
        MediaType::Mixed => "mixed",
    }
}

#[cfg(test)]
mod tests {
    use crate::jellyfin::api::model::BaseItemDto;
    use crate::library::StoredCredentials;

    use super::*;

    fn provider_title(id: u64, order: u32) -> NormalizedTitle {
        NormalizedTitle {
            identity: CanonicalIdentity::new(MediaType::Movie, id).expect("identity"),
            title: format!("Movie {id}"),
            original_title: None,
            year: None,
            overview: String::new(),
            release_date: None,
            source_order: order,
            poster_path: None,
            backdrop_path: None,
            adult: false,
        }
    }

    #[test]
    fn duplicate_local_editions_make_one_owned_card_with_a_chooser() {
        let library = Library::open_in_memory().expect("library");
        let credentials = StoredCredentials {
            server_id: Some("server".to_string()),
            user_id: Some("user".to_string()),
            ..library.credentials()
        };
        library.save_credentials(&credentials).expect("credentials");
        let items = [
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"edition-a","Name":"One","Type":"Movie","ProviderIds":{"Tmdb":"1"}}"#,
            )
            .expect("first"),
            serde_json::from_str::<BaseItemDto>(
                r#"{"Id":"edition-b","Name":"One 4K","Type":"Movie","ProviderIds":{"Tmdb":"1"}}"#,
            )
            .expect("second"),
        ];
        library.upsert_page(&items).expect("items");
        let account = AccountKey::new("server", "user").expect("account");
        let classified = classify(
            &library,
            &account,
            &[provider_title(1, 0), provider_title(2, 1)],
            OwnershipPolicy {
                complete_sync: true,
                restricted_user: false,
            },
        )
        .expect("classify");
        assert_eq!(classified.owned.len(), 1);
        assert_eq!(classified.owned[0].local_items.len(), 2);
        assert_eq!(classified.missing.len(), 1);
    }

    #[test]
    fn restricted_users_never_receive_missing_rows_or_counts() {
        let library = Library::open_in_memory().expect("library");
        let mut credentials = library.credentials();
        credentials.server_id = Some("server".to_string());
        credentials.user_id = Some("user".to_string());
        library.save_credentials(&credentials).expect("credentials");
        let account = AccountKey::new("server", "user").expect("account");
        let classified = classify(
            &library,
            &account,
            &[provider_title(2, 0)],
            OwnershipPolicy {
                complete_sync: true,
                restricted_user: true,
            },
        )
        .expect("classify");
        assert!(classified.missing.is_empty());
        assert!(classified.owned.is_empty());
        assert!(classified.ownership_available);
    }

    #[test]
    fn restricted_users_receive_no_provider_rows_while_ownership_is_unavailable() {
        let library = Library::open_in_memory().expect("library");
        let account = AccountKey::new("server", "user").expect("account");
        let classified = classify(
            &library,
            &account,
            &[provider_title(2, 0)],
            OwnershipPolicy {
                complete_sync: false,
                restricted_user: true,
            },
        )
        .expect("classify");
        assert!(classified.items.is_empty());
        assert!(!classified.ownership_available);
    }

    #[test]
    fn secondary_identity_mapping_classifies_an_item_without_tmdb() {
        let library = Library::open_in_memory().expect("library");
        let credentials = StoredCredentials {
            server_id: Some("server".to_string()),
            user_id: Some("user".to_string()),
            ..library.credentials()
        };
        library.save_credentials(&credentials).expect("credentials");
        let item = serde_json::from_str::<BaseItemDto>(
            r#"{"Id":"imdb-edition","Name":"One","Type":"Movie","ProviderIds":{"Imdb":"tt1234567"}}"#,
        )
        .expect("item");
        library.upsert_page(&[item]).expect("items");
        assert_eq!(
            unresolved_secondary_identities(&library).expect("unresolved"),
            [SecondaryIdentityRequest {
                media_type: MediaType::Movie,
                imdb_id: Some("tt1234567".to_string()),
                tvdb_id: None,
            }]
        );
        save_identity_mappings(
            &library,
            &[ResolvedIdentityMapping {
                media_type: MediaType::Movie,
                provider: "imdb".to_string(),
                provider_id: "tt1234567".to_string(),
                tmdb_id: 77,
            }],
        )
        .expect("mapping");
        let classified = classify(
            &library,
            &AccountKey::new("server", "user").expect("account"),
            &[provider_title(77, 0)],
            OwnershipPolicy {
                complete_sync: true,
                restricted_user: false,
            },
        )
        .expect("classify");
        assert_eq!(classified.owned[0].local_items[0].id, "imdb-edition");
        assert!(
            unresolved_secondary_identities(&library)
                .expect("resolved")
                .is_empty()
        );
    }
}
