use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::app::services::ShellBridge;
use crate::jellyfin::api::items;
use crate::jellyfin::session::Session;
use crate::library::{CatalogKind, ItemSummary, Library};

use super::fetch_page;

const CHILD_RECONCILE_INTERVAL: Duration = Duration::from_secs(60);
static CHILD_RECONCILES: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn claim_child_reconcile(parent_id: &str) -> bool {
    let now = Instant::now();
    let mut reconciles = CHILD_RECONCILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reconciles.retain(|_, started| now.duration_since(*started) < CHILD_RECONCILE_INTERVAL);
    if reconciles.contains_key(parent_id) {
        return false;
    }
    reconciles.insert(parent_id.to_string(), now);
    true
}

pub fn title_children(
    library: &Arc<Library>,
    session: &Arc<Session>,
    shell: &Arc<ShellBridge>,
    parent_id: &str,
    kind: CatalogKind,
) -> rusqlite::Result<Vec<ItemSummary>> {
    let cached = library.children(parent_id)?;
    if !matches!(kind, CatalogKind::Series | CatalogKind::Season) {
        return Ok(cached);
    }
    if !cached.is_empty() {
        if claim_child_reconcile(parent_id) {
            let (library, session, shell) =
                (Arc::clone(library), Arc::clone(session), Arc::clone(shell));
            let parent_id = parent_id.to_string();
            if let Err(error) = std::thread::Builder::new()
                .name("reconcile-children".to_string())
                .spawn(move || {
                    reconcile_children(&library, &session, &shell, &parent_id);
                })
            {
                tracing::warn!(target: "app.api", "could not start a child reconcile: {error}");
            }
        }
        return Ok(cached);
    }
    claim_child_reconcile(parent_id);
    let overviews = reconcile_children(library, session, shell, parent_id);
    let mut children = library.children(parent_id)?;
    if let Some(overviews) = &overviews {
        for child in &mut children {
            child.overview = overviews.get(&child.id).cloned().flatten();
        }
    }
    Ok(children)
}

fn reconcile_children(
    library: &Library,
    session: &Session,
    shell: &ShellBridge,
    parent_id: &str,
) -> Option<HashMap<String, Option<String>>> {
    let scope = session.scope().ok()?;

    let mut live_items = Vec::new();
    let mut overviews = HashMap::new();
    let mut offset = 0;
    loop {
        let page = match fetch_page(session, &scope, |client, user_id| {
            items::fetch_children(client, user_id, parent_id, offset)
        }) {
            Ok(page) => page,
            Err(error) => {
                tracing::debug!(
                    target: "app.api",
                    "could not reconcile the children of {parent_id}: {error}"
                );
                session.note_scoped_error(&scope, &error);
                return None;
            }
        };
        let received = page.items.len() as i64;
        if page.items.is_empty() {
            break;
        }
        for item in &page.items {
            overviews.insert(item.id.clone(), item.overview.clone());
        }
        live_items.extend(page.items);
        offset += received;
        if received < items::CHILDREN_PAGE_SIZE {
            break;
        }
    }

    let reconciled = session.commit_if_current(
        &scope,
        || (),
        |live| Ok(library.reconcile_children(live, parent_id, &live_items)),
    );
    let Ok(reconciled) = reconciled else {
        tracing::debug!(
            target: "app.api",
            parent_id,
            "skipped a child reconcile: the account changed while it ran"
        );
        return None;
    };
    match reconciled {
        Ok(changes) => {
            if !changes.is_empty() {
                tracing::info!(
                    target: "app.api",
                    changed = changes.item_ids.len(),
                    parent_id,
                    "reconciled changed child rows"
                );
                shell.library_changed(changes);
            }
        }
        Err(error) => {
            tracing::warn!(target: "app.api", "could not commit reconciled children: {error}");
            return None;
        }
    }
    Some(overviews)
}
