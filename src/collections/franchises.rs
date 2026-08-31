use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{CanonicalIdentity, ClassifiedTitle, NormalizedTitle};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FranchiseSnapshot {
    pub collection_id: u64,
    pub name: String,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub committed_at: i64,
    pub items: Vec<NormalizedTitle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FranchiseMembership {
    pub tmdb_id: u64,
    pub collection_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FranchiseView {
    pub collection_id: u64,
    pub name: String,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub owned: Vec<ClassifiedTitle>,
    pub missing: Vec<NormalizedTitle>,
}

/// Applies local-date visibility to cached provider membership. Owned titles
/// remain visible regardless of their date. Missing future or undated titles
/// are hidden unless the account setting opts in.
pub fn visible_franchises(
    snapshots: &[FranchiseSnapshot],
    local_items: &HashMap<CanonicalIdentity, Vec<super::LocalItem>>,
    include_unreleased: bool,
    local_date: &str,
) -> Vec<FranchiseView> {
    let today = parse_date(local_date);
    let mut views = snapshots
        .iter()
        .filter_map(|snapshot| visible_franchise(snapshot, local_items, include_unreleased, today))
        .collect::<Vec<_>>();
    views.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.collection_id.cmp(&right.collection_id))
    });
    views
}

fn visible_franchise(
    snapshot: &FranchiseSnapshot,
    local_items: &HashMap<CanonicalIdentity, Vec<super::LocalItem>>,
    include_unreleased: bool,
    today: Option<i64>,
) -> Option<FranchiseView> {
    let mut identities = HashSet::new();
    let mut owned = Vec::new();
    let mut missing = Vec::new();
    let mut items = snapshot.items.iter().collect::<Vec<_>>();
    items.sort_by_key(|item| release_order_key(item));
    for item in items {
        if item.adult || !identities.insert(item.identity.clone()) {
            continue;
        }
        if let Some(local) = local_items
            .get(&item.identity)
            .filter(|items| !items.is_empty())
        {
            owned.push(ClassifiedTitle {
                title: item.clone(),
                local_items: local.clone(),
            });
        } else if include_unreleased || released_by(item.release_date.as_deref(), today) {
            missing.push(item.clone());
        }
    }
    (!owned.is_empty() && owned.len() + missing.len() >= 2).then(|| FranchiseView {
        collection_id: snapshot.collection_id,
        name: snapshot.name.clone(),
        poster_path: snapshot.poster_path.clone(),
        backdrop_path: snapshot.backdrop_path.clone(),
        owned,
        missing,
    })
}

/// TMDB does not guarantee that collection parts follow release order.
pub(crate) fn sort_titles_by_release_date(items: &mut [NormalizedTitle]) {
    items.sort_by_key(release_order_key);
}

fn release_order_key(item: &NormalizedTitle) -> (bool, i64, u32, u64) {
    let date = item.release_date.as_deref().and_then(parse_date);
    (
        date.is_none(),
        date.unwrap_or_default(),
        item.source_order,
        item.identity.tmdb_id,
    )
}

fn released_by(value: Option<&str>, today: Option<i64>) -> bool {
    let (Some(date), Some(today)) = (value.and_then(parse_date), today) else {
        return false;
    };
    date <= today
}

fn parse_date(value: &str) -> Option<i64> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(year * 10_000 + month * 100 + day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{LocalItem, MediaType};

    fn title(id: u64, release_date: Option<&str>, order: u32) -> NormalizedTitle {
        NormalizedTitle {
            identity: CanonicalIdentity::new(MediaType::Movie, id).expect("identity"),
            title: format!("Movie {id}"),
            original_title: None,
            year: None,
            overview: String::new(),
            release_date: release_date.map(str::to_string),
            source_order: order,
            poster_path: None,
            backdrop_path: None,
            adult: false,
        }
    }

    fn local(identity: CanonicalIdentity) -> HashMap<CanonicalIdentity, Vec<LocalItem>> {
        HashMap::from([(
            identity,
            vec![LocalItem {
                id: "owned".to_string(),
                name: "Owned".to_string(),
                kind: "Movie".to_string(),
                played: false,
            }],
        )])
    }

    #[test]
    fn owned_titles_follow_release_date_instead_of_provider_order() {
        let dark_knight = title(155, Some("2008-07-16"), 0);
        let rises = title(49_026, Some("2012-07-17"), 1);
        let begins = title(272, Some("2005-06-10"), 2);
        let undated = title(99, None, 3);
        let snapshot = FranchiseSnapshot {
            collection_id: 263,
            name: "The Dark Knight Collection".to_string(),
            poster_path: None,
            backdrop_path: None,
            committed_at: 0,
            items: vec![dark_knight, rises, begins, undated],
        };
        let local = snapshot
            .items
            .iter()
            .map(|item| {
                (
                    item.identity.clone(),
                    vec![LocalItem {
                        id: format!("owned-{}", item.identity.tmdb_id),
                        name: item.title.clone(),
                        kind: "Movie".to_string(),
                        played: false,
                    }],
                )
            })
            .collect();

        let views = visible_franchises(&[snapshot], &local, false, "2026-08-30");
        let ids = views[0]
            .owned
            .iter()
            .map(|item| item.title.identity.tmdb_id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec![272, 155, 49_026, 99]);
    }

    #[test]
    fn future_missing_member_does_not_make_a_franchise_qualify() {
        let owned = title(1, Some("2024-01-01"), 0);
        let snapshot = FranchiseSnapshot {
            collection_id: 1,
            name: "F1".to_string(),
            poster_path: None,
            backdrop_path: None,
            committed_at: 0,
            items: vec![owned.clone(), title(2, Some("2027-01-01"), 1)],
        };
        let local = local(owned.identity);
        assert!(
            visible_franchises(std::slice::from_ref(&snapshot), &local, false, "2026-08-26")
                .is_empty()
        );
        assert_eq!(
            visible_franchises(&[snapshot], &local, true, "2026-08-26").len(),
            1
        );
    }

    #[test]
    fn cached_title_appears_at_its_local_release_date() {
        let owned = title(1, Some("2024-01-01"), 0);
        let snapshot = FranchiseSnapshot {
            collection_id: 2,
            name: "Alien".to_string(),
            poster_path: None,
            backdrop_path: None,
            committed_at: 0,
            items: vec![owned.clone(), title(2, Some("2026-08-26"), 1)],
        };
        let local = local(owned.identity);
        assert!(
            visible_franchises(std::slice::from_ref(&snapshot), &local, false, "2026-08-25")
                .is_empty()
        );
        assert_eq!(
            visible_franchises(&[snapshot], &local, false, "2026-08-26").len(),
            1
        );
    }
}
