//! Letterboxd's public-profile adapter.
//!
//! No credentials or page scraping are involved. Every public Letterboxd
//! member profile exposes an RSS feed. Connected profiles are fixed to the
//! canonical Letterboxd host before this module sees them, responses have a
//! hard byte ceiling, and review HTML is reduced to bounded plain text before
//! it crosses the native/UI boundary.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::library::ExternalProfile;

const MAX_RSS_BYTES: usize = 512 * 1024;
const MAX_REVIEW_CHARS: usize = 6_000;
const MAX_DISPLAY_NAME_CHARS: usize = 128;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const FEED_CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_CACHE_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Keeps one detail request from producing an unbounded fan-out if a local
/// database has been edited or imported from a future build.
pub const MAX_CONNECTED_PROFILES: usize = 16;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    status: VerificationStatus,
    display_name: Option<String>,
}

impl Verification {
    pub fn as_str(&self) -> &'static str {
        self.status.as_str()
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}

/// One connected member's newest rating and newest written review available
/// in the current RSS feed for a film. Review content is always plain text;
/// when a review is present, `entry_url` and `watched_date` describe that
/// review rather than a newer rating-only rewatch. The URL is present only
/// when it is a canonical film URL below that same member profile.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberReview {
    pub profile_id: String,
    pub username: String,
    pub display_name: String,
    pub profile_url: String,
    pub entry_url: Option<String>,
    pub rating: Option<f64>,
    pub review: Option<String>,
    pub review_truncated: bool,
    pub watched_date: Option<String>,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewLookup {
    pub reviews: Vec<MemberReview>,
    pub configured_profiles: usize,
    pub unavailable_profiles: usize,
}

#[derive(Debug, Clone)]
struct CachedFeed {
    fetched_at: Instant,
    profile_checked_at: Option<i64>,
    by_tmdb_id: HashMap<String, MemberReview>,
}

/// Process-local feed cache. The source is public, but the cache key still
/// carries the Jellyfin server and user ids so two signed-in accounts can never
/// consume one another's connected-profile selection.
#[derive(Debug, Default)]
pub struct ReviewService {
    feeds: Mutex<HashMap<String, CachedFeed>>,
}

impl ReviewService {
    pub fn reviews_for_item(&self, profiles: &[ExternalProfile], tmdb_id: &str) -> ReviewLookup {
        if !valid_tmdb_id(tmdb_id) {
            return ReviewLookup {
                reviews: Vec::new(),
                configured_profiles: 0,
                unavailable_profiles: 0,
            };
        }

        let enabled = profiles
            .iter()
            .filter(|profile| profile.provider == "letterboxd" && profile.enabled)
            .take(MAX_CONNECTED_PROFILES)
            .cloned()
            .collect::<Vec<_>>();
        if enabled.is_empty() {
            return ReviewLookup {
                reviews: Vec::new(),
                configured_profiles: 0,
                unavailable_profiles: 0,
            };
        }

        let pending = {
            let feeds = self
                .feeds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            enabled
                .iter()
                .filter(|profile| {
                    let key = profile_cache_key(profile);
                    !feeds.get(&key).is_some_and(|cached| {
                        cached.fetched_at.elapsed() < FEED_CACHE_TTL
                            && cached.profile_checked_at == profile.last_checked_at
                    })
                })
                .cloned()
                .collect::<Vec<_>>()
        };

        // At most MAX_CONNECTED_PROFILES short, bounded requests run together,
        // so one unavailable member cannot multiply the endpoint's 12-second
        // response budget by the number of connected friends.
        let refreshes = std::thread::scope(|scope| {
            pending
                .into_iter()
                .map(|profile| {
                    scope.spawn(move || {
                        let result = fetch_profile_feed(&profile);
                        (profile, result)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect::<Vec<_>>()
        });

        let mut failed = HashSet::new();
        let mut feeds = self
            .feeds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (profile, result) in refreshes {
            match result {
                Ok(by_tmdb_id) => {
                    feeds.insert(
                        profile_cache_key(&profile),
                        CachedFeed {
                            fetched_at: Instant::now(),
                            profile_checked_at: profile.last_checked_at,
                            by_tmdb_id,
                        },
                    );
                }
                Err(error) => {
                    tracing::debug!(
                        target: "letterboxd.rss",
                        profile = %profile.profile_key,
                        "could not refresh public profile feed: {error}"
                    );
                    failed.insert(profile.id);
                }
            }
        }
        feeds.retain(|_, cached| cached.fetched_at.elapsed() < MAX_CACHE_AGE);

        let mut reviews = Vec::new();
        for profile in &enabled {
            let Some(cached) = feeds.get(&profile_cache_key(profile)) else {
                continue;
            };
            let Some(review) = cached.by_tmdb_id.get(tmdb_id) else {
                continue;
            };
            let mut review = review.clone();
            review.stale = failed.contains(&profile.id);
            reviews.push(review);
        }

        ReviewLookup {
            reviews,
            configured_profiles: enabled.len(),
            unavailable_profiles: failed.len(),
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

/// Probe and parse the official RSS representation with short timeouts and a
/// hard read ceiling. A non-success response remains an unverified stored
/// profile so the user can correct a temporarily unavailable account without
/// retyping it.
pub fn verify(profile: &Profile) -> Verification {
    let Ok(xml) = fetch_rss(&profile.username) else {
        return Verification {
            status: VerificationStatus::Unverified,
            display_name: None,
        };
    };
    verification_from_xml(&xml)
}

fn fetch_profile_feed(profile: &ExternalProfile) -> Result<HashMap<String, MemberReview>, String> {
    let source = normalize_profile(&profile.profile_key)
        .map_err(|_| "stored profile is invalid".to_string())?;
    let xml = fetch_rss(&source.username)?;
    parse_feed(profile, &xml)
}

fn fetch_rss(username: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(format!("mediaflick-desktop/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let url = format!("https://letterboxd.com/{username}/rss/");
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|_| "Letterboxd did not return a public feed".to_string())?;
    if !(200..300).contains(&response.status().as_u16()) {
        return Err("Letterboxd did not return a public feed".to_string());
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit((MAX_RSS_BYTES + 1) as u64)
        .read_to_vec()
        .map_err(|_| "could not read the Letterboxd feed".to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_RSS_BYTES {
        return Err("Letterboxd feed is empty or too large".to_string());
    }
    String::from_utf8(bytes).map_err(|_| "Letterboxd feed is not UTF-8".to_string())
}

fn verification_from_xml(xml: &str) -> Verification {
    let Ok(document) = roxmltree::Document::parse(xml) else {
        return Verification {
            status: VerificationStatus::Unverified,
            display_name: None,
        };
    };
    let Some(channel) = rss_channel(&document) else {
        return Verification {
            status: VerificationStatus::Unverified,
            display_name: None,
        };
    };
    Verification {
        status: VerificationStatus::Verified,
        display_name: channel_display_name(channel),
    }
}

fn parse_feed(
    profile: &ExternalProfile,
    xml: &str,
) -> Result<HashMap<String, MemberReview>, String> {
    let source = normalize_profile(&profile.profile_key)
        .map_err(|_| "stored profile is invalid".to_string())?;
    let document = roxmltree::Document::parse(xml)
        .map_err(|_| "Letterboxd returned malformed RSS".to_string())?;
    let channel = rss_channel(&document)
        .ok_or_else(|| "Letterboxd returned an unexpected document".to_string())?;
    let display_name =
        channel_display_name(channel).unwrap_or_else(|| profile.display_name.clone());

    let mut by_tmdb_id = HashMap::new();
    for item in channel
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "item")
    {
        let Some(tmdb_id) = child_text(item, "movieId").map(str::trim) else {
            continue;
        };
        if !valid_tmdb_id(tmdb_id) {
            continue;
        }

        let rating = child_text(item, "memberRating")
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0 && *value <= 5.0);
        let guid = child_text(item, "guid").unwrap_or_default().trim();
        let (review, review_truncated) = if guid.starts_with("letterboxd-review-") {
            child_text(item, "description")
                .map(review_plain_text)
                .unwrap_or((None, false))
        } else {
            (None, false)
        };
        if rating.is_none() && review.is_none() {
            continue;
        }

        let entry_url = child_text(item, "link")
            .map(str::trim)
            .and_then(|url| canonical_entry_url(&source.username, url));
        let watched_date = child_text(item, "watchedDate")
            .map(str::trim)
            .filter(|value| valid_iso_date(value))
            .map(str::to_string);

        let activity = by_tmdb_id
            .entry(tmdb_id.to_string())
            .or_insert_with(|| MemberReview {
                profile_id: profile.id.clone(),
                username: source.username.clone(),
                display_name: display_name.clone(),
                profile_url: source.canonical_url.clone(),
                entry_url: None,
                rating: None,
                review: None,
                review_truncated: false,
                watched_date: None,
                stale: false,
            });

        // Letterboxd orders the feed newest first. Keep looking only until the
        // newest value for each independent surface has been found: a newer
        // rating-only rewatch must not erase an older written review that is
        // still present in this same bounded feed.
        let selects_newest_rating = activity.rating.is_none() && rating.is_some();
        if selects_newest_rating {
            activity.rating = rating;
        }
        if activity.review.is_none() && review.is_some() {
            activity.review = review;
            activity.review_truncated = review_truncated;
            activity.entry_url = entry_url;
            activity.watched_date = watched_date;
        } else if activity.review.is_none() && selects_newest_rating {
            // Until a written review is found, the newest rated entry remains
            // the click/date destination for rating-only activity.
            activity.entry_url = entry_url;
            activity.watched_date = watched_date;
        }
    }
    Ok(by_tmdb_id)
}

fn rss_channel<'a, 'input>(
    document: &'a roxmltree::Document<'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    (document.root_element().tag_name().name() == "rss")
        .then(|| {
            document
                .root_element()
                .children()
                .find(|node| node.is_element() && node.tag_name().name() == "channel")
        })
        .flatten()
}

/// Letterboxd identifies the public member behind a feed as
/// `<title>Letterboxd - {display name}</title>`. Keep the username as the
/// fallback if the provider ever changes that shape, and bound the external
/// string before persisting or returning it to the UI.
fn channel_display_name(channel: roxmltree::Node<'_, '_>) -> Option<String> {
    let title = channel
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "title")?
        .text()?;
    let display_name = title.strip_prefix("Letterboxd - ")?;
    let normalized = display_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!normalized.is_empty() && normalized.chars().count() <= MAX_DISPLAY_NAME_CHARS)
        .then_some(normalized)
}

fn child_text<'a, 'input>(item: roxmltree::Node<'a, 'input>, name: &str) -> Option<&'a str> {
    item.children()
        .find(|node| node.is_element() && node.tag_name().name() == name)
        .and_then(|node| node.text())
}

fn profile_cache_key(profile: &ExternalProfile) -> String {
    format!(
        "{}\0{}\0{}",
        profile.jellyfin_server_id, profile.jellyfin_user_id, profile.id
    )
}

fn valid_tmdb_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_iso_date(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

fn canonical_entry_url(username: &str, value: &str) -> Option<String> {
    let prefix = format!("https://letterboxd.com/{username}/film/");
    let remainder = value.strip_prefix(&prefix)?;
    (!remainder.is_empty()
        && value.len() <= 512
        && remainder
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/' | b'%')))
    .then(|| value.to_string())
}

/// The feed description is CDATA containing provider-sanitized HTML. Still,
/// never hand it to React: remove every tag and decode only text entities here.
fn review_plain_text(html: &str) -> (Option<String>, bool) {
    let mut text = String::with_capacity(html.len().min(MAX_REVIEW_CHARS));
    let mut rest = html;
    let mut discarding = false;
    while let Some(open) = rest.find('<') {
        if !discarding {
            text.push_str(&decode_html_entities(&rest[..open]));
        }
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find('>') else {
            break;
        };
        let tag = after_open[..close].trim().to_ascii_lowercase();
        let tag_name = tag
            .trim_start_matches('/')
            .split(|character: char| character.is_ascii_whitespace() || character == '/')
            .next()
            .unwrap_or_default();
        if matches!(tag_name, "script" | "style") {
            discarding = !tag.starts_with('/');
        } else if !discarding
            && (tag.starts_with("br")
                || tag.starts_with("/p")
                || tag.starts_with("/li")
                || tag.starts_with("/blockquote"))
        {
            text.push('\n');
        }
        rest = &after_open[close + 1..];
    }
    if !discarding {
        text.push_str(&decode_html_entities(rest));
    }

    let normalized = text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .filter(|line| !(line.starts_with("Watched on ") && line.ends_with('.')))
        .collect::<Vec<_>>()
        .join("\n\n");
    if normalized.is_empty() {
        return (None, false);
    }

    if normalized.chars().count() <= MAX_REVIEW_CHARS {
        return (Some(normalized), false);
    }
    let mut truncated = normalized
        .chars()
        .take(MAX_REVIEW_CHARS)
        .collect::<String>();
    while truncated.ends_with(char::is_whitespace) {
        truncated.pop();
    }
    truncated.push('…');
    (Some(truncated), true)
}

fn decode_html_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative_ampersand) = value[cursor..].find('&') {
        let ampersand = cursor + relative_ampersand;
        output.push_str(&value[cursor..ampersand]);
        let tail = &value[ampersand + 1..];
        let Some(relative_semicolon) = tail.find(';').filter(|index| *index <= 12) else {
            output.push('&');
            cursor = ampersand + 1;
            continue;
        };
        let entity = &tail[..relative_semicolon];
        if let Some(decoded) = decode_html_entity(entity) {
            output.push_str(&decoded);
            cursor = ampersand + relative_semicolon + 2;
        } else {
            output.push('&');
            cursor = ampersand + 1;
        }
    }
    output.push_str(&value[cursor..]);
    output
}

fn decode_html_entity(entity: &str) -> Option<String> {
    let named = match entity {
        "amp" => Some("&"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "quot" => Some("\""),
        "apos" | "#039" => Some("'"),
        "nbsp" => Some(" "),
        "hellip" => Some("…"),
        "ndash" => Some("–"),
        "mdash" => Some("—"),
        "lsquo" => Some("‘"),
        "rsquo" => Some("’"),
        "ldquo" => Some("“"),
        "rdquo" => Some("”"),
        _ => None,
    };
    if let Some(named) = named {
        return Some(named.to_string());
    }
    let codepoint = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
        .and_then(|value| u32::from_str_radix(value, 16).ok())
        .or_else(|| {
            entity
                .strip_prefix('#')
                .and_then(|value| value.parse::<u32>().ok())
        })?;
    char::from_u32(codepoint).map(|character| character.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        MemberReview, canonical_entry_url, normalize_profile, parse_feed, verification_from_xml,
    };
    use crate::library::ExternalProfile;

    fn profile() -> ExternalProfile {
        ExternalProfile {
            id: "profile-1".to_string(),
            provider: "letterboxd".to_string(),
            profile_key: "alice".to_string(),
            display_name: "alice".to_string(),
            canonical_url: "https://letterboxd.com/alice/".to_string(),
            enabled: true,
            verification_status: "verified".to_string(),
            created_at: 1,
            last_checked_at: Some(2),
            jellyfin_server_id: "server".to_string(),
            jellyfin_user_id: "user".to_string(),
        }
    }

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

    #[test]
    fn reads_and_normalizes_the_member_display_name_from_the_channel_title() {
        let verification = verification_from_xml(
            r#"<rss><channel><title>Letterboxd -   Alice &amp; Bob  </title></channel></rss>"#,
        );
        assert_eq!(verification.as_str(), "verified");
        assert_eq!(verification.display_name(), Some("Alice & Bob"));
    }

    #[test]
    fn parses_rating_and_plain_text_review_by_tmdb_id() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:letterboxd="https://letterboxd.com" xmlns:tmdb="https://themoviedb.org">
  <channel>
    <title>Letterboxd - Alice Film Fan</title>
    <item>
      <link>https://letterboxd.com/alice/film/the-matrix/</link>
      <guid isPermaLink="false">letterboxd-review-1</guid>
      <letterboxd:watchedDate>2026-08-04</letterboxd:watchedDate>
      <letterboxd:memberRating>4.5</letterboxd:memberRating>
      <tmdb:movieId>603</tmdb:movieId>
      <description><![CDATA[<p><img src="poster.jpg"/></p><p>Smart &amp; stylish. &#x1F49A;</p><p><em>Still</em> rules.</p>]]></description>
    </item>
  </channel>
</rss>"#;
        let reviews = parse_feed(&profile(), xml).expect("feed");
        assert_eq!(
            reviews.get("603"),
            Some(&MemberReview {
                profile_id: "profile-1".to_string(),
                username: "alice".to_string(),
                display_name: "Alice Film Fan".to_string(),
                profile_url: "https://letterboxd.com/alice/".to_string(),
                entry_url: Some("https://letterboxd.com/alice/film/the-matrix/".to_string()),
                rating: Some(4.5),
                review: Some("Smart & stylish. 💚\n\nStill rules.".to_string()),
                review_truncated: false,
                watched_date: Some("2026-08-04".to_string()),
                stale: false,
            })
        );
    }

    #[test]
    fn combines_the_newest_rating_with_an_older_review_in_the_same_feed() {
        let xml = r#"<rss xmlns:letterboxd="https://letterboxd.com" xmlns:tmdb="https://themoviedb.org"><channel>
          <item><link>https://letterboxd.com/alice/film/movie/1/</link><guid>letterboxd-watch-2</guid><letterboxd:watchedDate>2026-08-03</letterboxd:watchedDate><letterboxd:memberRating>3.5</letterboxd:memberRating><tmdb:movieId>42</tmdb:movieId><description><![CDATA[<p><img src="poster"/></p><p>Watched on Monday August 3, 2026.</p>]]></description></item>
          <item><link>https://letterboxd.com/alice/film/movie/</link><guid>letterboxd-review-1</guid><letterboxd:watchedDate>2026-07-12</letterboxd:watchedDate><letterboxd:memberRating>2.0</letterboxd:memberRating><tmdb:movieId>42</tmdb:movieId><description><![CDATA[<p>Older words.</p>]]></description></item>
        </channel></rss>"#;
        let reviews = parse_feed(&profile(), xml).expect("feed");
        let latest = reviews.get("42").expect("latest");
        assert_eq!(latest.rating, Some(3.5));
        assert_eq!(latest.review.as_deref(), Some("Older words."));
        assert_eq!(
            latest.entry_url.as_deref(),
            Some("https://letterboxd.com/alice/film/movie/")
        );
        assert_eq!(latest.watched_date.as_deref(), Some("2026-07-12"));
    }

    #[test]
    fn combines_a_newest_review_with_the_newest_older_rating() {
        let xml = r#"<rss xmlns:letterboxd="https://letterboxd.com" xmlns:tmdb="https://themoviedb.org"><channel>
          <item><link>https://letterboxd.com/alice/film/movie/2/</link><guid>letterboxd-review-2</guid><letterboxd:watchedDate>2026-08-03</letterboxd:watchedDate><tmdb:movieId>42</tmdb:movieId><description><![CDATA[<p>Newest words.</p>]]></description></item>
          <item><link>https://letterboxd.com/alice/film/movie/1/</link><guid>letterboxd-watch-1</guid><letterboxd:watchedDate>2026-07-12</letterboxd:watchedDate><letterboxd:memberRating>4.0</letterboxd:memberRating><tmdb:movieId>42</tmdb:movieId></item>
        </channel></rss>"#;
        let reviews = parse_feed(&profile(), xml).expect("feed");
        let latest = reviews.get("42").expect("latest");
        assert_eq!(latest.rating, Some(4.0));
        assert_eq!(latest.review.as_deref(), Some("Newest words."));
        assert_eq!(
            latest.entry_url.as_deref(),
            Some("https://letterboxd.com/alice/film/movie/2/")
        );
        assert_eq!(latest.watched_date.as_deref(), Some("2026-08-03"));
    }

    #[test]
    fn does_not_borrow_an_older_link_for_a_newer_rating_only_entry() {
        let xml = r#"<rss xmlns:letterboxd="https://letterboxd.com" xmlns:tmdb="https://themoviedb.org"><channel>
          <item><link>https://example.test/not-canonical</link><guid>letterboxd-watch-2</guid><letterboxd:watchedDate>2026-08-03</letterboxd:watchedDate><letterboxd:memberRating>3.5</letterboxd:memberRating><tmdb:movieId>42</tmdb:movieId></item>
          <item><link>https://letterboxd.com/alice/film/movie/1/</link><guid>letterboxd-watch-1</guid><letterboxd:watchedDate>2026-07-12</letterboxd:watchedDate><letterboxd:memberRating>2.0</letterboxd:memberRating><tmdb:movieId>42</tmdb:movieId></item>
        </channel></rss>"#;
        let reviews = parse_feed(&profile(), xml).expect("feed");
        let latest = reviews.get("42").expect("latest");
        assert_eq!(latest.rating, Some(3.5));
        assert_eq!(latest.entry_url, None);
        assert_eq!(latest.watched_date.as_deref(), Some("2026-08-03"));
    }

    #[test]
    fn accepts_only_the_connected_members_canonical_film_links() {
        assert!(canonical_entry_url("alice", "https://example.test/alice/film/movie/").is_none());
        assert!(canonical_entry_url("alice", "https://letterboxd.com/bob/film/movie/").is_none());
        assert!(
            canonical_entry_url("alice", "https://letterboxd.com/alice/film/movie/?next=bad")
                .is_none()
        );
        assert_eq!(
            canonical_entry_url("alice", "https://letterboxd.com/alice/film/movie/2/").as_deref(),
            Some("https://letterboxd.com/alice/film/movie/2/")
        );
    }
}
