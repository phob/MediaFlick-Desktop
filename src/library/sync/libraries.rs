use std::collections::HashSet;

use crate::jellyfin::api::{ApiError, items};
use crate::jellyfin::session::{Session, SessionScope};
use crate::library::catalog_libraries::MembershipReplaced;
use crate::library::libraries::{LibraryId, LibraryRegistry, MediaLibrary};
use crate::library::{Library, TitleKind};

use super::{
    IDENTITY_PAGE_SIZE, MAX_IDENTITY_PAGES, SyncHandle, best_effort, cancelled, commit,
    storage_error,
};

pub(super) struct LibraryList {
    pub(super) libraries: Vec<MediaLibrary>,
}

impl LibraryList {
    pub(super) fn lists_no_catalog_library(&self) -> bool {
        self.libraries
            .iter()
            .all(|entry| entry.catalog_title().is_none())
    }
}

/// Announces a changed list as soon as it is stored, so the sidebar does not
/// wait for the catalog walk behind it.
pub(super) fn refresh_libraries(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    control: Option<&SyncHandle>,
) -> Result<LibraryList, ApiError> {
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
    let changed = commit(session, scope, || {
        library
            .replace_libraries(&libraries)
            .map_err(|error| storage_error(&error))
    })?;
    if changed
        && session.scope_is_current(scope)
        && let Some(control) = control
    {
        tracing::debug!(target: "library.sync", "announced a changed library list");
        control.shell().libraries_changed();
    }
    Ok(LibraryList { libraries })
}

/// Returns whether a title joined or left a library. Announces a settled walk
/// even when a later library cancels the pass.
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
    let due = libraries
        .into_iter()
        .filter(|entry| entry.membership_pending)
        .filter_map(|entry| Some((entry.catalog_title()?, entry.id)));
    let mut pass = MembershipReplaced::default();
    let outcome = (|| -> Result<(), ApiError> {
        for (title, id) in due {
            let walked = refresh_membership(library, session, scope, &id, title, control);
            let rate_limited = matches!(walked, Err(ApiError::RateLimited { .. }));
            let step = format!("the membership of library {}", id.as_str());
            pass.merge(best_effort(&step, walked)?.unwrap_or_default());
            if rate_limited {
                break;
            }
        }
        Ok(())
    })();
    if pass.settled
        && session.scope_is_current(scope)
        && let Some(control) = control
    {
        tracing::debug!(target: "library.sync", "announced settled library memberships");
        control.shell().library_memberships_settled();
    }
    outcome.map(|()| pass.changed)
}

fn refresh_membership(
    library: &Library,
    session: &Session,
    scope: &SessionScope,
    library_id: &LibraryId,
    title: TitleKind,
    control: Option<&SyncHandle>,
) -> Result<MembershipReplaced, ApiError> {
    let Some(ids) = member_ids(session, scope, library_id, title, control)? else {
        return Ok(MembershipReplaced::default());
    };
    if ids.is_empty() {
        tracing::debug!(target: "library.sync", library = library_id.as_str(), "the server listed no titles in a library; keeping its membership");
    }
    commit(session, scope, || {
        library
            .replace_membership(library_id, &ids)
            .map_err(|error| storage_error(&error))
    })
}

fn member_ids(
    session: &Session,
    scope: &SessionScope,
    library_id: &LibraryId,
    title: TitleKind,
    control: Option<&SyncHandle>,
) -> Result<Option<HashSet<String>>, ApiError> {
    let mut ids = HashSet::new();
    let mut offset = 0;
    let mut server_count = None;
    for _ in 0..MAX_IDENTITY_PAGES {
        cancelled(control, session, scope)?;
        let page = items::fetch_library_member_ids(
            scope.client(),
            scope.user_id(),
            library_id.as_str(),
            title.into(),
            offset,
            IDENTITY_PAGE_SIZE,
        )?;
        let counted = *server_count.get_or_insert(page.total_record_count);
        let received = page.items.len() as i64;
        ids.extend(
            page.items
                .into_iter()
                .map(|item| item.id)
                .filter(|id| !id.trim().is_empty()),
        );
        if received < IDENTITY_PAGE_SIZE {
            if (ids.len() as i64) < counted {
                tracing::warn!(
                    target: "library.sync",
                    library = library_id.as_str(),
                    listed = ids.len(),
                    counted,
                    "a library membership walk listed fewer titles than the server counted; keeping its membership"
                );
                return Ok(None);
            }
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
