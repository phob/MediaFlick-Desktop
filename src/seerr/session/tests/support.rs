use super::*;

pub(super) fn library() -> Arc<Library> {
    Arc::new(Library::open_in_memory().expect("library"))
}

/// A throwaway HTTP server answering one canned response per request and
/// recording the request heads it saw. The cookie plumbing is the part of
/// this milestone that only a real socket can prove.
pub(super) fn fake_server(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let base_url = format!("http://{}", listener.local_addr().expect("address"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    std::thread::spawn(move || {
        for (stream, response) in listener.incoming().zip(responses) {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut head = String::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if line == "\r\n" => break,
                    Ok(_) => head.push_str(&line),
                }
            }
            // Drain the request body before answering: closing a socket
            // that still holds unread data resets the connection, and the
            // client sees that as a transport failure rather than the
            // response it was just sent. It is kept on the end of the head
            // so a test can assert on what was actually sent.
            let length = head
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if length > 0 {
                let mut body = vec![0u8; length];
                if reader.read_exact(&mut body).is_ok() {
                    head.push_str(&String::from_utf8_lossy(&body));
                }
            }
            seen.lock().expect("lock").push(head);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (base_url, requests)
}

/// `Connection: close` keeps one request to one connection, so the canned
/// responses line up with the requests one for one.
pub(super) fn response(status: &str, body: &str, cookies: &[&str]) -> String {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for cookie in cookies {
        head.push_str(&format!("Set-Cookie: {cookie}\r\n"));
    }
    format!("{head}\r\n{body}")
}

/// What a forward-auth proxy answers with: a 302 towards its own sign-on
/// flow, with an HTML body nothing should be reading.
pub(super) fn redirect_response(location: &str) -> String {
    let body = "<html><body>Found.</body></html>";
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\n\
         Content-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
}

/// What a proxy, a sign-on page, or Seerr's own front end answers with.
pub(super) fn html_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub(super) const INITIALIZED: &str = r#"{"initialized":true,"applicationTitle":"Seerr","mediaServerType":2,
    "localLogin":true,"mediaServerLogin":true,"newPlexLogin":true,"movie4kEnabled":false,
    "series4kEnabled":false,"partialRequestsEnabled":true}"#;
pub(super) const VERSION: &str = r#"{"version":"3.3.0","commitTag":"local"}"#;
/// `/auth/me` for a Seerr account backed by the Jellyfin user `uid`.
pub(super) const ME: &str =
    r#"{"id":7,"displayName":"pho","jellyfinUserId":"uid","permissions":32}"#;
pub(super) const QUOTA: &str = r#"{"movie":{"used":0},"tv":{"used":0}}"#;
/// The `Set-Cookie` an established Seerr session arrives on.
pub(super) const SESSION: &str = "connect.sid=s%3Aabc.def; Path=/; HttpOnly";

pub(super) fn signed_in(library: &Library, user_id: &str) {
    signed_in_to(library, "http://server:8096", user_id);
}

pub(super) fn signed_in_to(library: &Library, server_url: &str, user_id: &str) {
    let mut credentials = library.credentials();
    credentials.server_url = Some(server_url.to_string());
    credentials.user_id = Some(user_id.to_string());
    credentials.server_id = Some("srv".to_string());
    credentials.token = Some("tok".to_string());
    library.save_credentials(&credentials).expect("credentials");
}

/// An instance that has been connected to but not linked — the state
/// `POST /api/seerr/connect` leaves behind.
pub(super) fn configured(library: &Library, base_url: &str) {
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some(base_url.to_string()),
            ..SeerrConfig::default()
        })
        .expect("seerr config");
}

/// The link as the session itself sees it, once the account guard has run.
/// Deliberately not `status()`: that confirms a live link against the
/// instance, which these tests have no server for.
pub(super) fn is_linked(session: &SeerrSession) -> bool {
    session.revalidate();
    session.read().is_linked()
}

pub(super) fn linked(library: &Library, jellyfin_user_id: &str) {
    linked_to(library, "https://seerr.test", jellyfin_user_id);
}

/// An established link, as the link flow leaves it: a session cookie, the
/// Seerr account, and the Jellyfin account it is bound to.
pub(super) fn linked_to(library: &Library, base_url: &str, jellyfin_user_id: &str) {
    library
        .save_seerr_config(&SeerrConfig {
            base_url: Some(base_url.to_string()),
            cookies: Some(r#"{"connect.sid":"abc"}"#.to_string()),
            user_id: Some(7),
            user_name: Some("pho".to_string()),
            jellyfin_server_id: Some("srv".to_string()),
            jellyfin_user_id: Some(jellyfin_user_id.to_string()),
            partial_requests_enabled: true,
            ..SeerrConfig::default()
        })
        .expect("seerr config");
}

/// Request bodies are pretty-printed on the wire, so what they contain is
/// asserted against a whitespace-free form of the whole recorded request.
pub(super) fn compact(request: &str) -> String {
    request.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A signed-in machine with a live Seerr link against `base_url`.
pub(super) fn session_linked_to(base_url: &str) -> (Arc<Library>, SeerrSession) {
    let library = library();
    signed_in(&library, "uid");
    linked_to(&library, base_url, "uid");
    let session = SeerrSession::restore(library.clone());
    (library, session)
}
