use std::collections::HashSet;
use std::time::Duration;

use serde::Deserialize;

use crate::jellyfin::playback_reporter::PlaybackSession;
use crate::mpv::MpvLaunch;

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentType {
    Intro,
    Outro,
    Recap,
    Commercial,
}

impl SegmentType {
    pub fn prompt_text(self) -> &'static str {
        match self {
            Self::Intro => "Seek to Skip Intro",
            Self::Outro => "Seek to Skip Credits",
            Self::Recap => "Seek to Skip Recap",
            Self::Commercial => "Seek to Skip Commercial",
        }
    }

    pub fn skipped_text(self) -> &'static str {
        match self {
            Self::Intro => "Skipped Intro",
            Self::Outro => "Skipped Credits",
            Self::Recap => "Skipped Recap",
            Self::Commercial => "Skipped Commercial",
        }
    }

    pub fn countdown_label(self) -> &'static str {
        match self {
            Self::Intro => "Intro",
            Self::Outro => "Credits",
            Self::Recap => "Recap",
            Self::Commercial => "Commercial",
        }
    }

    pub fn marker_start_label(self) -> &'static str {
        match self {
            Self::Intro => "Intro",
            Self::Outro => "Credits",
            Self::Recap => "Recap",
            Self::Commercial => "Commercial",
        }
    }

    pub fn marker_end_label(self) -> &'static str {
        match self {
            Self::Intro => "Intro End",
            Self::Outro => "Credits End",
            Self::Recap => "Recap End",
            Self::Commercial => "Commercial End",
        }
    }

    fn from_jellyfin(value: &str) -> Option<Self> {
        match value {
            value if value.eq_ignore_ascii_case("Intro") => Some(Self::Intro),
            value if value.eq_ignore_ascii_case("Outro") => Some(Self::Outro),
            value if value.eq_ignore_ascii_case("Credits") => Some(Self::Outro),
            value if value.eq_ignore_ascii_case("Recap") => Some(Self::Recap),
            value if value.eq_ignore_ascii_case("Commercial") => Some(Self::Commercial),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipSegment {
    pub segment_type: SegmentType,
    pub start_ticks: i64,
    pub end_ticks: i64,
    pub triggered: bool,
}

#[derive(Debug, Deserialize)]
struct MediaSegmentsResponse {
    #[serde(default, rename = "Items")]
    items: Vec<MediaSegmentDto>,
}

#[derive(Debug, Deserialize)]
struct MediaSegmentDto {
    #[serde(rename = "Type")]
    segment_type: String,
    #[serde(default, rename = "StartTicks")]
    start_ticks: i64,
    #[serde(default, rename = "EndTicks")]
    end_ticks: i64,
}

pub fn fetch_for_launch(launch: &MpvLaunch) -> Result<Vec<SkipSegment>, String> {
    let session = PlaybackSession::from_launch(launch)
        .ok_or_else(|| "missing Jellyfin session details for media segments".to_string())?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .user_agent(format!("mediaflick-desktop/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();

    let mut attempted = HashSet::<String>::new();
    let mut last_empty = None;
    let mut last_error = None;
    for item_id in [session.media_source_id(), Some(session.item_id())]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
    {
        if !attempted.insert(item_id.to_string()) {
            continue;
        }
        match fetch_segments(&agent, &session, item_id) {
            Ok(segments) if !segments.is_empty() => return Ok(segments),
            Ok(segments) => last_empty = Some(segments),
            Err(error) => last_error = Some(error),
        }
    }

    if let Some(segments) = last_empty {
        Ok(segments)
    } else {
        Err(last_error.unwrap_or_else(|| "no Jellyfin item id for media segments".to_string()))
    }
}

fn fetch_segments(
    agent: &ureq::Agent,
    session: &PlaybackSession,
    item_id: &str,
) -> Result<Vec<SkipSegment>, String> {
    let url = format!(
        "{}/MediaSegments/{}?includeSegmentTypes=Intro&includeSegmentTypes=Outro&includeSegmentTypes=Recap&includeSegmentTypes=Commercial",
        session.base_url().trim_end_matches('/'),
        encode_path_segment(item_id)
    );
    tracing::debug!(
        target: "jellyfin.media_segments",
        item_id,
        "fetching Jellyfin media segments"
    );

    let mut request = agent.get(url.as_str()).header("Accept", "application/json");
    for header in session.auth_headers() {
        request = request.header(header.name.as_str(), header.value.as_str());
    }

    let mut response = request.call().map_err(|error| error.to_string())?;
    let body = response
        .body_mut()
        .read_json::<MediaSegmentsResponse>()
        .map_err(|error| error.to_string())?;
    let mut segments = body
        .items
        .into_iter()
        .filter_map(|item| {
            let segment_type = SegmentType::from_jellyfin(&item.segment_type)?;
            (item.end_ticks > item.start_ticks && item.start_ticks >= 0).then_some(SkipSegment {
                segment_type,
                start_ticks: item.start_ticks,
                end_ticks: item.end_ticks,
                triggered: false,
            })
        })
        .collect::<Vec<_>>();
    segments.sort_by_key(|segment| segment.start_ticks);
    tracing::debug!(
        target: "jellyfin.media_segments",
        item_id,
        count = segments.len(),
        "fetched Jellyfin media segments"
    );
    Ok(segments)
}

fn encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{SegmentType, encode_path_segment};

    #[test]
    fn maps_jellyfin_segment_types_including_recap_and_commercial() {
        assert_eq!(
            SegmentType::from_jellyfin("Intro"),
            Some(SegmentType::Intro)
        );
        assert_eq!(
            SegmentType::from_jellyfin("Outro"),
            Some(SegmentType::Outro)
        );
        assert_eq!(
            SegmentType::from_jellyfin("Credits"),
            Some(SegmentType::Outro)
        );
        assert_eq!(
            SegmentType::from_jellyfin("recap"),
            Some(SegmentType::Recap)
        );
        assert_eq!(
            SegmentType::from_jellyfin("COMMERCIAL"),
            Some(SegmentType::Commercial)
        );
        assert_eq!(SegmentType::from_jellyfin("Preview"), None);
    }

    #[test]
    fn passes_through_guid_item_ids() {
        let id = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
        assert_eq!(encode_path_segment(id), id);
    }

    #[test]
    fn encodes_path_and_query_separators() {
        assert_eq!(encode_path_segment("../Users"), "..%2FUsers");
        assert_eq!(
            encode_path_segment("x?includeSegmentTypes=Outro"),
            "x%3FincludeSegmentTypes%3DOutro"
        );
    }
}
