use serde_json::json;

pub fn error_toast_script(title: &str, body: &str) -> String {
    let payload = json!({ "title": title, "body": body });
    include_str!("error_toast.js").replace("{{error_payload}}", &payload.to_string())
}
