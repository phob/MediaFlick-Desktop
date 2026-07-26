//! Device profile sent with `PlaybackInfo`.
//!
//! Both supported players are full desktop decoders, so the profile declares
//! unrestricted direct play and only offers a transcoding fallback when the
//! user picked a capped streaming quality. Modelled on jellyfin-mpv-shim's
//! profile (see `.agent-source.json`).

use serde_json::{Value, json};

use crate::app::build_info;
use crate::preferences::StreamingQuality;

/// Bitrate advertised when the user asked for untouched original files.
const UNLIMITED_BITRATE: u64 = 1_000_000_000;

pub fn device_profile(quality: StreamingQuality) -> Value {
    let max_bitrate = quality.max_streaming_bitrate().unwrap_or(UNLIMITED_BITRATE);
    json!({
        "Name": build_info::APP_NAME,
        "MaxStreamingBitrate": max_bitrate,
        "MaxStaticBitrate": max_bitrate,
        "MusicStreamingTranscodingBitrate": 1_280_000,
        "DirectPlayProfiles": [
            { "Type": "Video" },
            { "Type": "Audio" },
        ],
        "TranscodingProfiles": transcoding_profiles(quality),
        "ContainerProfiles": [],
        "CodecProfiles": [],
        "SubtitleProfiles": subtitle_profiles(),
    })
}

fn transcoding_profiles(quality: StreamingQuality) -> Value {
    if !quality.allows_transcoding() {
        return json!([]);
    }
    json!([
        {
            "Protocol": "hls",
            "Container": "ts",
            "Type": "Video",
            "AudioCodec": "aac,mp3,ac3,eac3",
            "VideoCodec": "h264,hevc",
            "Context": "Streaming",
            "MaxAudioChannels": "6",
            "MinSegments": "1",
            "BreakOnNonKeyFrames": true,
        },
        {
            "Container": "mp3",
            "Type": "Audio",
            "AudioCodec": "mp3",
            "Context": "Streaming",
            "Protocol": "http",
        },
    ])
}

/// Text subtitles are handed to the player as separate files; image-based
/// formats stay in the container because neither player can fetch them.
fn subtitle_profiles() -> Value {
    json!([
        { "Format": "srt", "Method": "External" },
        { "Format": "subrip", "Method": "External" },
        { "Format": "ass", "Method": "External" },
        { "Format": "ssa", "Method": "External" },
        { "Format": "vtt", "Method": "External" },
        { "Format": "sub", "Method": "External" },
        { "Format": "smi", "Method": "External" },
        { "Format": "pgssub", "Method": "Embed" },
        { "Format": "dvdsub", "Method": "Embed" },
        { "Format": "dvbsub", "Method": "Embed" },
    ])
}

#[cfg(test)]
mod tests {
    use super::{UNLIMITED_BITRATE, device_profile};
    use crate::preferences::StreamingQuality;

    #[test]
    fn original_quality_advertises_no_transcoding() {
        let profile = device_profile(StreamingQuality::Original);
        assert_eq!(profile["MaxStreamingBitrate"], UNLIMITED_BITRATE);
        assert_eq!(profile["TranscodingProfiles"].as_array().unwrap().len(), 0);
        assert_eq!(profile["DirectPlayProfiles"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn capped_quality_offers_an_hls_fallback_at_that_bitrate() {
        let profile = device_profile(StreamingQuality::Mbps10);
        assert_eq!(profile["MaxStreamingBitrate"], 10_000_000);
        let transcoding = profile["TranscodingProfiles"].as_array().unwrap();
        assert_eq!(transcoding[0]["Protocol"], "hls");
        assert_eq!(transcoding[0]["Container"], "ts");
    }

    #[test]
    fn auto_quality_transcodes_without_a_bitrate_cap() {
        let profile = device_profile(StreamingQuality::Auto);
        assert_eq!(profile["MaxStreamingBitrate"], UNLIMITED_BITRATE);
        assert!(
            !profile["TranscodingProfiles"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn text_subtitles_are_delivered_externally() {
        let profile = device_profile(StreamingQuality::Original);
        let subtitles = profile["SubtitleProfiles"].as_array().unwrap();
        let srt = subtitles
            .iter()
            .find(|entry| entry["Format"] == "srt")
            .expect("srt profile");
        assert_eq!(srt["Method"], "External");
        let pgs = subtitles
            .iter()
            .find(|entry| entry["Format"] == "pgssub")
            .expect("pgssub profile");
        assert_eq!(pgs["Method"], "Embed");
    }
}
