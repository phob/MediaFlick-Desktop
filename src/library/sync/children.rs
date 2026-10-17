use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::app::services::ShellBridge;
use crate::jellyfin::api::items;
use crate::jellyfin::session::Session;
use crate::library::{CatalogKind, ItemSummary, Library};

/// How long one parent's reconciled child list is trusted before a later
/// navigation asks the server again. It also absorbs the refetch that the
/// reconcile's own change notification triggers.
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

/// A cached title's children. A series or season is also reconciled with the
/// server.
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
    // The synced catalog already holds every season and episode, so a cached
    // list answers at once and the reconcile runs behind it; its change
    // notification refreshes the page if the server disagreed. Only a
    // container with nothing cached yet waits for the server.
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

/// Re-reads one parent's child list from the server.
///
/// The cache alone cannot be trusted on a detail page: deleting episodes in
/// Jellyfin leaves their rows behind until the next identity sweep, and the
/// season view is exactly where those ghosts surface — a wall of art-less cards
/// with a dead Play button. The image-404 eviction only cleans up rows whose
/// poster happens to be requested, so it misses lazily-loaded cards below the
/// fold and episodes that never had artwork.
///
/// One small non-recursive request per navigation buys a correct list, and it
/// also makes newly added episodes appear without waiting for a sweep. Changed
/// rows reach an open page through the library change notification.
///
/// Returns each live child's synopsis so the response can carry it without the
/// cache ever storing prose; `None` means the server could not be asked.
fn reconcile_children(
    library: &Library,
    session: &Session,
    shell: &ShellBridge,
    parent_id: &str,
) -> Option<HashMap<String, Option<String>>> {
    let scope = session.scope().ok()?;
    let (client, user_id) = (scope.client(), scope.user_id());

    let mut live_items = Vec::new();
    let mut overviews = HashMap::new();
    let mut offset = 0;
    loop {
        let page = match items::fetch_children(client, user_id, parent_id, offset) {
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

    // An empty `live_items` here came from a successful request, so it is the server
    // saying this parent has no children left — unlike the library-wide sweep,
    // where the blast radius makes that answer too dangerous to trust.
    // The list is the server's answer for the account that asked.
    let reconciled = session.commit_if_current(
        &scope,
        || (),
        |_live| Ok(library.reconcile_children(parent_id, &live_items)),
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
