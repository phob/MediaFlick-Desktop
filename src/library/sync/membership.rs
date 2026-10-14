//! Which libraries the user sees, and which cached titles each movies or TV
//! shows library holds. Both are replaced whole by idempotent passes; item
//! existence stays with the identity sweep.

use std::collections::HashSet;

use crate::jellyfin::api::{ApiError, items};
use crate::jellyfin::session::{Session, SessionScope};
use crate::library::libraries::{GridSource, LibraryId, LibraryRegistry, MediaLibrary};
use crate::library::{CatalogKind, Library, LibrarySetChange};

use super::cycle::{cancelled, commit, storage_error};
use super::{IDENTITY_PAGE_SIZE, MAX_IDENTITY_PAGES, SyncHandle};

/// Stores the server's library list. A successful empty answer is the
/// server's and is stored as such.
pub(super) fn refresh_libraries(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
) -> Result<LibrarySetChange, ApiError> {
    let views = items::fetch_user_views(scope.client(), scope.user_id())?;
    let mut seen = HashSet::new();
    let libraries = views
        .iter()
        .filter_map(|view| {
            let parsed = MediaLibrary::from_view(view);
            if parsed.is_none() {
                tracing::warn!(target: "library.sync", id = view.id, "skipped a library with an unusable id");
            }
            parsed
        })
        .filter(|parsed| seen.insert(parsed.id.clone()))
        .collect::<Vec<_>>();
    commit(session, scope, || {
        library
            .replace_libraries(&libraries)
            .map_err(|error| storage_error(&error))
    })
}

/// Replaces each catalog library's membership with the ids the server lists
/// in it now. A walk cut off at the page cap, or an empty answer, keeps that
/// library's previous membership: neither may empty a grid. Returns whether
/// any membership changed.
pub(super) fn membership_pass(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    control: Option<&SyncHandle>,
) -> Result<bool, ApiError> {
    let registry = library.libraries().map_err(|error| storage_error(&error))?;
    let LibraryRegistry::Known(libraries) = registry else {
        return Ok(false);
    };
    let mut changed = false;
    for entry in &libraries {
        let GridSource::Catalog { title } = entry.kind().profile().grid else {
            continue;
        };
        let Some(ids) = member_ids(session, scope, &entry.id, title, control)? else {
            continue;
        };
        if ids.is_empty() {
            tracing::debug!(target: "library.sync", library = entry.id.as_str(), "the server listed no titles in a library; keeping its membership");
            continue;
        }
        changed |= commit(session, scope, || {
            library
                .replace_membership(&entry.id, &ids)
                .map_err(|error| storage_error(&error))
        })?;
    }
    Ok(changed)
}

/// `None` when the walk stopped at the page cap.
fn member_ids(
    session: &Session,
    scope: &SessionScope,
    library_id: &LibraryId,
    kind: CatalogKind,
    control: Option<&SyncHandle>,
) -> Result<Option<HashSet<String>>, ApiError> {
    let mut ids = HashSet::new();
    let mut offset = 0;
    for _ in 0..MAX_IDENTITY_PAGES {
        cancelled(control, session, scope)?;
        let page = items::fetch_library_member_ids(
            scope.client(),
            scope.user_id(),
            library_id.as_str(),
            kind,
            offset,
            IDENTITY_PAGE_SIZE,
        )?;
        let received = page.items.len() as i64;
        ids.extend(
            page.items
                .into_iter()
                .map(|item| item.id)
                .filter(|id| !id.trim().is_empty()),
        );
        if received < IDENTITY_PAGE_SIZE {
            return Ok(Some(ids));
        }
        offset += received;
    }
    tracing::warn!(
        target: "library.sync",
        library = library_id.as_str(),
        offset,
        "stopped a library membership walk at the page cap; keeping its membership"
    );
    Ok(None)
}
