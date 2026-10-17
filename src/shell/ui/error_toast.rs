/// The error toast, showing `toast`, a `{ title, body }` object as JSON.
pub fn error_toast_script(toast: &str) -> String {
    include_str!("error_toast.js").replace("{{error_payload}}", toast)
}
