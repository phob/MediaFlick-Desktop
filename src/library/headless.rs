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
    println!(
        "bootstrap:   {}",
        if library.meta("sync.bootstrap_done").as_deref() == Some("1") {
            "complete"
        } else {
            "in progress"
        }
    );
    if let Some(report) = library.meta("sync.last_report") {
        println!("last sync:   {report}");
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
    match sync::run_cycle(&library, &session) {
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
