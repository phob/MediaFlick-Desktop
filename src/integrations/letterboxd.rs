//! Letterboxd's public-profile adapter.
//!
//! No credentials or scraping are involved.  Every public Letterboxd member
//! profile exposes an RSS feed, so a bounded request to that feed gives the
//! first version a useful verification signal while keeping the persisted
//! model independent from any future approved API client.

use std::time::Duration;

const MAX_RSS_BYTES: usize = 512 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub username: String,
    pub canonical_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    Verified,
    Unverified,
}

impl VerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Unverified => "unverified",
        }
    }
}

/// Accept either a member username or only a canonical Letterboxd profile URL.
/// The fixed URL stored after this step means no user-provided host is ever
/// passed to an external browser or HTTP client.
pub fn normalize_profile(input: &str) -> Result<Profile, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("enter a Letterboxd username or profile URL".to_string());
    }
    let username = if input.contains("://") {
        let lower = input.to_ascii_lowercase();
        let prefix = [
            "https://letterboxd.com/",
            "http://letterboxd.com/",
            "https://www.letterboxd.com/",
            "http://www.letterboxd.com/",
        ]
        .into_iter()
        .find(|prefix| lower.starts_with(prefix))
        .ok_or_else(|| "use a letterboxd.com profile URL".to_string())?;
        let remainder = &input[prefix.len()..];
        let username = remainder.trim_end_matches('/');
        if username.contains('/') || username.contains('?') || username.contains('#') {
            return Err("use a member profile URL, not a list or review URL".to_string());
        }
        username
    } else {
        input
    };
    if username.len() > 64
        || username.is_empty()
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(
            "Letterboxd usernames may only contain letters, numbers, hyphens, and underscores"
                .to_string(),
        );
    }
    let username = username.to_ascii_lowercase();
    Ok(Profile {
        canonical_url: format!("https://letterboxd.com/{username}/"),
        username,
    })
}

/// Probe the official RSS representation with short timeouts and a hard read
/// ceiling. A non-success response remains an unverified stored profile so the
/// user can correct a temporarily unavailable account without retyping it.
pub fn verify(profile: &Profile) -> VerificationStatus {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(format!("mediaflick-desktop/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let url = format!("https://letterboxd.com/{}/rss/", profile.username);
    let Ok(mut response) = agent.get(&url).call() else {
        return VerificationStatus::Unverified;
    };
    if !(200..300).contains(&response.status().as_u16()) {
        return VerificationStatus::Unverified;
    }
    match response
        .body_mut()
        .with_config()
        .limit(MAX_RSS_BYTES as u64)
        .read_to_vec()
    {
        Ok(bytes) if !bytes.is_empty() => VerificationStatus::Verified,
        _ => VerificationStatus::Unverified,
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_profile;

    #[test]
    fn normalizes_a_username_and_canonical_url() {
        let profile = normalize_profile("Pho-Flick").expect("profile");
        assert_eq!(profile.username, "pho-flick");
        assert_eq!(profile.canonical_url, "https://letterboxd.com/pho-flick/");
        let url = normalize_profile("https://www.letterboxd.com/Pho-Flick/").expect("url");
        assert_eq!(url, profile);
    }

    #[test]
    fn refuses_non_profile_hosts_and_paths() {
        assert!(normalize_profile("https://example.test/phoflick/").is_err());
        assert!(normalize_profile("https://letterboxd.com/phoflick/films/").is_err());
        assert!(normalize_profile("../../../oops").is_err());
    }
}
