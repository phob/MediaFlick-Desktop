pub fn error_toast_script(toast: &str) -> String {
    include_str!("error_toast.js").replace("{{error_payload}}", toast)
}
