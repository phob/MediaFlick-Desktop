pub fn spawn_named(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new()
        .name(name.to_string())
        .spawn(work)
    {
        tracing::warn!(target: "app", thread = name, "failed to start a background thread: {error}");
    }
}
