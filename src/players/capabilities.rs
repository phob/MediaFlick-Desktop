use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilities {
    pub hw_video_codecs: Vec<String>,
    pub supports_10bit_hevc: bool,
    pub supports_10bit_av1: bool,
    pub supports_hdr10: bool,
    pub max_audio_channels: u8,
    pub passthrough_codecs: Vec<String>,
}

impl HostCapabilities {
    pub fn detect() -> &'static Self {
        static DETECTED: OnceLock<HostCapabilities> = OnceLock::new();
        DETECTED.get_or_init(probe_host_capabilities)
    }

    #[cfg(test)]
    pub fn for_test() -> Self {
        Self {
            hw_video_codecs: vec!["h264".into(), "hevc".into(), "vp9".into(), "av1".into()],
            supports_10bit_hevc: true,
            supports_10bit_av1: true,
            supports_hdr10: true,
            max_audio_channels: 6,
            passthrough_codecs: default_passthrough_codecs(),
        }
    }
}

pub fn default_passthrough_codecs() -> Vec<String> {
    vec![
        "ac3".into(),
        "eac3".into(),
        "dts".into(),
        "dts-hd".into(),
        "truehd".into(),
    ]
}

pub fn parse_vainfo_profiles(output: &str) -> Vec<String> {
    let mut codecs = Vec::new();
    for line in output.lines() {
        let Some((profile, entrypoint)) = line.split_once(':') else {
            continue;
        };
        if !entrypoint.contains("VAEntrypointVLD") {
            continue;
        }
        let profile = profile.trim();
        let mut push = |codec: &str| {
            if !codecs.iter().any(|existing| existing == codec) {
                codecs.push(codec.to_string());
            }
        };
        if profile.starts_with("VAProfileH264") {
            push("h264");
        } else if profile == "VAProfileHEVCMain10" {
            push("hevc");
            push("hevc10");
        } else if profile == "VAProfileHEVCMain" || profile.starts_with("VAProfileHEVCRext") {
            push("hevc");
        } else if profile.starts_with("VAProfileVP9Profile") {
            push("vp9");
            if profile == "VAProfileVP9Profile2" || profile == "VAProfileVP9Profile3" {
                push("vp9_10");
            }
        } else if profile.starts_with("VAProfileAV1Profile") {
            push("av1");
        }
    }
    codecs
}

pub fn parse_pactl_max_channels(output: &str) -> u8 {
    let mut best = 0_u8;
    for line in output.lines() {
        for token in line.split_whitespace() {
            if let Some(prefix) = token.strip_suffix("ch")
                && let Ok(count) = prefix.parse::<u8>()
                && (1..=16).contains(&count)
            {
                best = best.max(count);
            }
        }
    }
    if best == 0 { 2 } else { best.clamp(2, 8) }
}

fn run_bounded_probe(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(400);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut stdout = child.stdout.take()?;
                let mut text = String::new();
                let _ = stdout.read_to_string(&mut text).ok()?;
                return Some(text);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn has_dri_render_node() -> bool {
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("renderD"))
}

#[cfg(target_os = "linux")]
fn probe_host_capabilities() -> HostCapabilities {
    let has_nvidia = std::path::Path::new("/dev/nvidia0").exists();
    let has_dri = has_dri_render_node();
    let mut hw_video_codecs = run_bounded_probe("vainfo", &[])
        .map(|output| parse_vainfo_profiles(&output))
        .unwrap_or_default();
    if hw_video_codecs.is_empty() && has_nvidia {
        hw_video_codecs = vec![
            "h264".into(),
            "hevc".into(),
            "hevc10".into(),
            "vp9".into(),
            "av1".into(),
        ];
    }
    let supports_10bit_hevc = hw_video_codecs.iter().any(|c| c == "hevc10") || has_nvidia;
    let supports_10bit_av1 = hw_video_codecs.iter().any(|c| c == "av1") || has_nvidia;
    let max_audio_channels = run_bounded_probe("pactl", &["list", "short", "sinks"])
        .map_or(2, |output| parse_pactl_max_channels(&output));
    let _ = has_dri;
    HostCapabilities {
        hw_video_codecs,
        supports_10bit_hevc,
        supports_10bit_av1,
        supports_hdr10: supports_10bit_hevc || supports_10bit_av1,
        max_audio_channels,
        passthrough_codecs: default_passthrough_codecs(),
    }
}

#[cfg(not(target_os = "linux"))]
fn probe_host_capabilities() -> HostCapabilities {
    let _ = run_bounded_probe;
    let _ = parse_vainfo_profiles;
    let _ = parse_pactl_max_channels;
    HostCapabilities {
        hw_video_codecs: vec!["h264".into(), "hevc".into(), "hevc10".into(), "vp9".into()],
        supports_10bit_hevc: true,
        supports_10bit_av1: true,
        supports_hdr10: true,
        max_audio_channels: 6,
        passthrough_codecs: default_passthrough_codecs(),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_pactl_max_channels, parse_vainfo_profiles};

    #[test]
    fn parses_vaapi_decode_profiles_and_ignores_encode_only_entries() {
        let vainfo = "\
vainfo: VA-API version: 1.20 (libva 2.20.0)
vainfo: Driver version: Intel iHD driver for Intel(R) Gen Graphics - 24.1.0 ()
vainfo: Supported profile and entrypoints
      VAProfileH264Main               : VAEntrypointVLD
      VAProfileH264High               : VAEntrypointVLD
      VAProfileHEVCMain               : VAEntrypointVLD
      VAProfileHEVCMain10             : VAEntrypointEncSlice
      VAProfileVP9Profile0            : VAEntrypointVLD
      VAProfileAV1Profile0            : VAEntrypointVLD
";
        let codecs = parse_vainfo_profiles(vainfo);
        assert!(codecs.contains(&"h264".to_string()), "got {codecs:?}");
        assert!(codecs.contains(&"hevc".to_string()), "got {codecs:?}");
        assert!(codecs.contains(&"vp9".to_string()), "got {codecs:?}");
        assert!(codecs.contains(&"av1".to_string()), "got {codecs:?}");
        assert!(
            !codecs.contains(&"hevc10".to_string()),
            "encode-only HEVC Main10 must not count as decode: {codecs:?}"
        );

        let with_main10_decode =
            format!("{vainfo}      VAProfileHEVCMain10             : VAEntrypointVLD\n");
        let codecs_10 = parse_vainfo_profiles(&with_main10_decode);
        assert!(codecs_10.contains(&"hevc10".to_string()));
    }

    #[test]
    fn parses_pactl_sink_channel_counts_and_clamps_to_two_through_eight() {
        let sinks = "\
52\talsa_output.pci-0000_00_1f.3.hdmi-stereo\tPipeWire\ts16le 2ch 48000Hz\tRUNNING
53\talsa_output.usb-Receiver.iec958-stereo\tPipeWire\ts24le 8ch 192000Hz\tIDLE
";
        assert_eq!(parse_pactl_max_channels(sinks), 8);

        let mono = "10\tmono-sink\tPipeWire\ts16le 1ch 44100Hz\tIDLE\n";
        assert_eq!(parse_pactl_max_channels(mono), 2);

        let surround = "11\tavr\tPipeWire\tfloat32le 12ch 48000Hz\tIDLE\n";
        assert_eq!(parse_pactl_max_channels(surround), 8);
        assert_eq!(parse_pactl_max_channels(""), 2);
    }
}
