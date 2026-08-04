//! Headless entry points for verifying the cache without starting CEF.

use std::path::Path;
use std::sync::Arc;

use crate::app::paths;
use crate::jellyfin::session::Session;

use super::{Library, sync};

/// Backs `--library-stats`: prints what the local cache currently holds.
pub fn print_stats() -> i32 {
    let Some(library) = open() else {
        return 1;
    };
    let credentials = library.credentials();
    let stats = library.stats();
    println!("database:    {}", library.path().display());
    println!(
        "server:      {}",
        credentials
            .server_url
            .as_deref()
            .unwrap_or("not configured")
    );
    println!(
        "user:        {}",
        credentials.user_name.as_deref().unwrap_or("signed out")
    );
    println!("device id:   {}", credentials.device_id);
    println!("movies:      {}", stats.movies);
    println!("series:      {}", stats.series);
    println!("seasons:     {}", stats.seasons);
    println!("episodes:    {}", stats.episodes);
    println!("total items: {}", stats.total);
    let catalog = sync::bootstrap_progress(&library);
    println!(
        "catalog:     {} · {} / {}",
        if catalog.complete {
            "complete"
        } else if catalog.ready {
            "usable, filling"
        } else {
            "waiting for first page"
        },
        catalog.processed,
        catalog
            .total
            .map_or_else(|| "unknown".to_string(), |total| total.to_string())
    );
    let enrichment = library.enrichment_diagnostics();
    println!(
        "enrichment:  complete {} / {} · pending {} · failed {} · next {:?}",
        enrichment.completed,
        enrichment.total,
        enrichment.pending,
        enrichment.failed,
        enrichment.next_due_at
    );
    if let Some(report) = library.meta("sync.last_report") {
        println!("last sync:   {report}");
    }
    let convergence = library.convergence_diagnostics();
    println!(
        "convergence: pending {} · due {} · dormant {} · oldest {:?} · next {:?}",
        convergence.pending,
        convergence.due,
        convergence.dormant,
        convergence.oldest_pending_at,
        convergence.next_due_at
    );
    if let Some(report) = convergence.last_run {
        println!(
            "last converge: selected {} · requests {} · completed {} · progressed {} · unchanged {} · dormant {} · failed {} · {} ms",
            report.selected,
            report.requests,
            report.completed,
            report.progressed,
            report.unchanged,
            report.dormant,
            report.failed,
            report.elapsed_ms
        );
    }
    0
}

/// Backs `--library-sync-once`: runs a single sync cycle synchronously.
pub fn sync_once() -> i32 {
    let Some(library) = open() else {
        return 1;
    };
    let library = Arc::new(library);
    let session = Session::restore(library.clone());
    if !session.is_authenticated() {
        eprintln!("not signed in: start the app and sign in first");
        return 2;
    }
    // Running this by hand is an explicit ask, so it reconciles deletions even
    // if the background thread swept less than an hour ago.
    match sync::run_cycle(&library, &session, sync::Trigger::Requested) {
        Ok(report) => {
            println!(
                "bootstrapped {} · updated {} · user data {} · deleted {} · {} ms",
                report.bootstrapped,
                report.updated,
                report.user_data_refreshed,
                report.deleted,
                report.elapsed_ms
            );
            0
        }
        Err(error) => {
            eprintln!("sync failed: {error}");
            1
        }
    }
}

fn open() -> Option<Library> {
    let path = paths::library_db_path();
    match Library::open(Path::new(&path)) {
        Ok(library) => Some(library),
        Err(error) => {
            eprintln!("could not open {}: {error}", path.display());
            None
        }
    }
}
