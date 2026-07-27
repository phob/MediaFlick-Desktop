//! Headless entry point for verifying the Seer link without starting CEF.

use std::path::Path;
use std::sync::Arc;

use crate::app::paths;
use crate::library::Library;

use super::SeerSession;

/// Backs `--seer-status`: prints the stored link and a live probe of it.
pub fn print_status() -> i32 {
    let path = paths::library_db_path();
    let library = match Library::open(Path::new(&path)) {
        Ok(library) => Arc::new(library),
        Err(error) => {
            eprintln!("could not open {}: {error}", path.display());
            return 1;
        }
    };

    let credentials = library.credentials();
    println!("database:    {}", library.path().display());
    println!(
        "jellyfin:    {}",
        credentials.user_name.as_deref().unwrap_or("signed out")
    );

    let session = SeerSession::restore(library);
    let status = session.status();
    let Some(server_url) = status["serverUrl"].as_str() else {
        println!("seer:        not configured");
        return 2;
    };
    println!("seer:        {server_url}");
    println!(
        "link:        {}",
        match (
            status["linked"].as_bool().unwrap_or(false),
            status["expired"].as_bool().unwrap_or(false),
        ) {
            (true, _) => format!(
                "linked as {}",
                status["user"]["name"].as_str().unwrap_or("unknown")
            ),
            (false, true) => "expired — re-link required".to_string(),
            (false, false) => "not linked".to_string(),
        }
    );
    println!(
        "instance:    4K movies {} · 4K series {} · season requests {}",
        on_off(status["instance"]["movie4kEnabled"].as_bool()),
        on_off(status["instance"]["series4kEnabled"].as_bool()),
        on_off(status["instance"]["partialRequestsEnabled"].as_bool()),
    );
    if let Some(capabilities) = status["capabilities"].as_object() {
        for kind in ["movie", "tv", "movie4k", "tv4k"] {
            let Some(capability) = capabilities.get(kind) else {
                continue;
            };
            println!(
                "{:<12} request {} · auto-approve {}",
                format!("{kind}:"),
                on_off(capability["request"].as_bool()),
                on_off(capability["autoApprove"].as_bool()),
            );
        }
    }
    if let Some(quota) = status["quota"].as_object() {
        for kind in ["movie", "tv"] {
            let Some(entry) = quota.get(kind) else {
                continue;
            };
            println!(
                "{:<12} {} used, {}",
                format!("{kind} quota:"),
                entry["used"].as_i64().unwrap_or(0),
                match entry["limit"].as_i64() {
                    Some(limit) => format!("limit {limit}"),
                    None => "unlimited".to_string(),
                }
            );
        }
    }

    // A configured instance that answers nothing useful is the failure worth
    // reporting as one, so the exit code is usable from a script.
    if status["linked"].as_bool().unwrap_or(false) {
        0
    } else {
        2
    }
}

fn on_off(value: Option<bool>) -> &'static str {
    if value.unwrap_or(false) { "on" } else { "off" }
}
