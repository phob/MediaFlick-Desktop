//! Named background threads.

/// Starts `work` on a thread called `name`, so it can be told apart in a
/// debugger, a crash report, or a profiler. A thread that cannot start is
/// logged and its work skipped, as for the other background services.
pub fn spawn_named(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new()
        .name(name.to_string())
        .spawn(work)
    {
        tracing::warn!(target: "app", thread = name, "failed to start a background thread: {error}");
    }
}
