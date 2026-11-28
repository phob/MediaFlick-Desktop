use serde_json::{Value, json};

use crate::app::build_info;
use crate::players::capabilities::HostCapabilities;
use crate::preferences::StreamingQuality;

const UNLIMITED_BITRATE: u64 = 1_000_000_000;

const DIRECT_PLAY_VIDEO_CODECS: &str = "h264,hevc,av1,vp9,vp8,mpeg2video,mpeg4,vc1";
const DIRECT_PLAY_AUDIO_CODECS: &str =
    "aac,ac3,eac3,dts,truehd,flac,opus,vorbis,mp3,alac,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le";

pub fn device_profile(quality: StreamingQuality) -> Value {
    device_profile_for(quality, HostCapabilities::detect(), false)
}

pub fn device_profile_for(
    quality: StreamingQuality,
    caps: &HostCapabilities,
    audio_passthrough: bool,
) -> Value {
    let max_bitrate = quality.max_streaming_bitrate().unwrap_or(UNLIMITED_BITRATE);
    json!({
        "Name": build_info::APP_NAME,
        "MaxStreamingBitrate": max_bitrate,
        "MaxStaticBitrate": max_bitrate,
        "MusicStreamingTranscodingBitrate": 1_280_000,
        "DirectPlayProfiles": [
            {
                "Type": "Video",
                "VideoCodec": DIRECT_PLAY_VIDEO_CODECS,
                "AudioCodec": DIRECT_PLAY_AUDIO_CODECS,
            },
            {
                "Type": "Audio",
                "AudioCodec": DIRECT_PLAY_AUDIO_CODECS,
            },
        ],
        "TranscodingProfiles": transcoding_profiles(quality, caps, audio_passthrough),
        "ContainerProfiles": [],
        "CodecProfiles": codec_profiles(caps),
        "SubtitleProfiles": subtitle_profiles(),
    })
}

fn transcode_video_codecs(caps: &HostCapabilities) -> String {
    let mut codecs = Vec::new();
    for preferred in ["av1", "hevc", "h264"] {
        if caps.hw_video_codecs.iter().any(|c| c == preferred) {
            codecs.push(preferred);
        }
    }
    for baseline in ["h264", "hevc"] {
        if !codecs.contains(&baseline) {
            codecs.push(baseline);
        }
    }
    codecs.join(",")
}

fn codec_profiles(caps: &HostCapabilities) -> Value {
    let hevc_profiles = if caps.supports_10bit_hevc {
        "Main|Main 10"
    } else {
        "Main"
    };
    let mut profiles = vec![
        json!({
            "Type": "Video",
            "Codec": "h264",
            "Conditions": [{
                "Condition": "LessThanEqual",
                "Property": "VideoLevel",
                "Value": "52",
                "IsRequired": false,
            }],
        }),
        json!({
            "Type": "Video",
            "Codec": "hevc",
            "Conditions": [{
                "Condition": "EqualsAny",
                "Property": "VideoProfile",
                "Value": hevc_profiles,
                "IsRequired": false,
            }],
        }),
    ];
    if caps.supports_10bit_av1 {
        profiles.push(json!({
            "Type": "Video",
            "Codec": "av1",
            "Conditions": [{
                "Condition": "LessThanEqual",
                "Property": "VideoBitDepth",
                "Value": "10",
                "IsRequired": false,
            }],
        }));
    }
    Value::Array(profiles)
}

fn transcoding_profiles(
    quality: StreamingQuality,
    caps: &HostCapabilities,
    audio_passthrough: bool,
) -> Value {
    if !quality.allows_transcoding() {
        return json!([]);
    }
    let max_channels = if audio_passthrough {
        caps.max_audio_channels.max(6)
    } else {
        caps.max_audio_channels
    }
    .clamp(2, 8);
    let audio_codec = if audio_passthrough {
        "ac3,eac3,dts,aac,mp3"
    } else {
        "aac,opus,flac,mp3,ac3,eac3"
    };
    json!([
        {
            "Protocol": "hls",
            "Container": "ts",
            "Type": "Video",
            "AudioCodec": audio_codec,
            "VideoCodec": transcode_video_codecs(caps),
            "Context": "Streaming",
            "MaxAudioChannels": max_channels.to_string(),
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
    use super::device_profile;
    use crate::preferences::StreamingQuality;

    #[test]
    fn original_quality_advertises_no_transcoding() {
        let profile = device_profile(StreamingQuality::Original);
        assert_eq!(profile["TranscodingProfiles"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn capped_quality_offers_an_hls_fallback_at_that_bitrate() {
        let profile = device_profile(StreamingQuality::Mbps10);
        assert_eq!(profile["MaxStreamingBitrate"], 10_000_000);
        assert!(
            !profile["TranscodingProfiles"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn device_profile_respects_host_channels_and_audio_passthrough() {
        let mut caps = crate::players::capabilities::HostCapabilities::for_test();
        caps.max_audio_channels = 2;
        caps.supports_10bit_av1 = false;
        caps.hw_video_codecs = vec!["h264".into()];

        let stereo = super::device_profile_for(StreamingQuality::Mbps10, &caps, false);
        assert_eq!(stereo["TranscodingProfiles"][0]["MaxAudioChannels"], "2");
        assert_eq!(
            stereo["TranscodingProfiles"][0]["AudioCodec"],
            "aac,opus,flac,mp3,ac3,eac3"
        );
        assert_eq!(stereo["TranscodingProfiles"][0]["VideoCodec"], "h264,hevc");
        let audio_codecs = stereo["DirectPlayProfiles"]
            .as_array()
            .expect("direct play profiles")[1]["AudioCodec"]
            .as_str()
            .expect("audio codecs");
        assert!(audio_codecs.contains("aac"));
        assert!(audio_codecs.contains("truehd"));

        caps.max_audio_channels = 8;
        caps.supports_10bit_av1 = true;
        caps.hw_video_codecs = vec!["av1".into(), "hevc".into(), "h264".into()];
        let passthrough = super::device_profile_for(StreamingQuality::Mbps20, &caps, true);
        assert_eq!(
            passthrough["TranscodingProfiles"][0]["MaxAudioChannels"],
            "8"
        );
        assert_eq!(
            passthrough["TranscodingProfiles"][0]["AudioCodec"],
            "ac3,eac3,dts,aac,mp3"
        );
        assert_eq!(
            passthrough["TranscodingProfiles"][0]["VideoCodec"],
            "av1,hevc,h264"
        );
        assert!(
            passthrough["CodecProfiles"]
                .as_array()
                .expect("codec profiles")
                .iter()
                .any(|p| p["Codec"] == "av1")
        );
    }
}
