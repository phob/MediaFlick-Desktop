use super::{
    PlayOptions, apply_saved_preference, build, choose_stream, embedded_ordinal, select_source,
};
use crate::jellyfin::api::JellyfinClient;
use crate::jellyfin::api::model::{MediaSourceInfo, PlaybackInfoResponse};
use crate::library::model::ResolvedPlaybackPreference;
use crate::preferences::StreamingQuality;

#[test]
fn resume_rewind_never_changes_explicit_positions_or_seeks_before_zero() {
    let mut options = PlayOptions {
        resume: true,
        ..Default::default()
    };
    options.viewing.resume_rewind_seconds = 10;
    assert_eq!(super::resume_start_ticks(&options, 150_000_000), 50_000_000);
    assert_eq!(super::resume_start_ticks(&options, 50_000_000), 0);
    options.start_ticks = Some(150_000_000);
    assert_eq!(
        super::resume_start_ticks(&options, 500_000_000),
        150_000_000
    );
    options.start_ticks = None;
    options.resume = false;
    assert_eq!(super::resume_start_ticks(&options, 500_000_000), 0);
}

#[test]
fn language_defaults_choose_ranked_audio_and_forced_subtitles() {
    let source = media_source(
        r#"{"Id":"source", "MediaStreams":[
            {"Type":"Audio","Index":1,"Language":"eng"},
            {"Type":"Audio","Index":2,"Language":"jpn","Title":"Original"},
            {"Type":"Subtitle","Index":3,"Language":"eng"},
            {"Type":"Subtitle","Index":4,"Language":"eng","IsForced":true}
        ]}"#,
    );
    let mut options = PlayOptions::default();
    options.viewing.audio_languages = vec!["de".into(), "en".into()];
    options.viewing.subtitle_languages = vec!["en".into()];
    options.viewing.subtitle_mode = crate::preferences::SubtitleMode::Forced;
    super::apply_language_defaults(&mut options, &source);
    assert_eq!(options.audio_stream_index, Some(1));
    assert_eq!(options.subtitle_stream_index, Some(4));
    options.viewing.prefer_original_audio = true;
    super::apply_language_defaults(&mut options, &source);
    assert_eq!(options.audio_stream_index, Some(2));
    options.viewing.prefer_original_audio = false;
    options.viewing.subtitle_mode = crate::preferences::SubtitleMode::ForeignAudio;
    super::apply_language_defaults(&mut options, &source);
    assert_eq!(options.subtitle_stream_index, Some(-1));
}

fn client() -> JellyfinClient {
    JellyfinClient::new("http://server:8096", "device-1", Some("secret"))
}

fn info(json: &str) -> PlaybackInfoResponse {
    serde_json::from_str(json).expect("playback info")
}

fn media_source(json: &str) -> MediaSourceInfo {
    serde_json::from_str(json).expect("media source")
}

fn options() -> PlayOptions {
    PlayOptions {
        item_id: "item-1".to_string(),
        ..Default::default()
    }
}

#[test]
fn direct_stream_urls_carry_the_session_but_not_the_token() {
    let response = info(
        r#"{"PlaySessionId":"session","MediaSources":[{"Id":"src","Container":"mkv",
                "SupportsDirectStream":true,"ETag":"etag","RunTimeTicks":120000000}]}"#,
    );
    let prepared = build(
        &client(),
        &response,
        StreamingQuality::Original,
        &options(),
        0,
        None,
    )
    .expect("prepared");
    assert_eq!(prepared.play_method, "DirectStream");
    assert!(!prepared.request.media_url.contains("api_key"));
    assert!(prepared.request.media_url.contains("mediaSourceId=src"));
    assert!(prepared.request.media_url.contains("playSessionId=session"));
    assert!(
        prepared
            .request
            .headers
            .iter()
            .any(|header| header.name == "X-Emby-Token")
    );
}

#[test]
fn direct_play_is_reported_separately_from_direct_stream() {
    let response =
        info(r#"{"MediaSources":[{"Id":"src","Container":"mp4","SupportsDirectPlay":true}]}"#);
    let prepared = build(
        &client(),
        &response,
        StreamingQuality::Original,
        &options(),
        0,
        None,
    )
    .expect("prepared");
    assert_eq!(prepared.play_method, "DirectPlay");
}

#[test]
fn transcoding_urls_are_resolved_against_the_server_root_without_the_token() {
    let response = info(
        r#"{"MediaSources":[{"Id":"src","SupportsTranscoding":true,
                "TranscodingUrl":"/videos/item-1/master.m3u8?DeviceId=d&ApiKey=secret&MediaSourceId=src&api_key=secret&PlaySessionId=p&X-Emby-Token=secret&ApiKeyHint=kept"}]}"#,
    );
    let prepared = build(
        &client(),
        &response,
        StreamingQuality::Mbps10,
        &options(),
        0,
        None,
    )
    .expect("prepared");
    assert_eq!(prepared.play_method, "Transcode");
    assert_eq!(
        prepared.request.media_url,
        "http://server:8096/videos/item-1/master.m3u8?DeviceId=d&MediaSourceId=src&PlaySessionId=p&ApiKeyHint=kept"
    );
    assert!(prepared.request.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("X-Emby-Token") && header.value == "secret"
    }));
}

#[test]
fn the_requested_media_source_wins_over_the_first_playable_one() {
    let response = info(
        r#"{"MediaSources":[{"Id":"a","SupportsDirectStream":true},
                                {"Id":"b","SupportsDirectStream":true}]}"#,
    );
    assert_eq!(
        select_source(&response, Some("b"), None).and_then(|source| source.id.clone()),
        Some("b".to_string())
    );
    assert_eq!(
        select_source(&response, Some("missing"), None).and_then(|source| source.id.clone()),
        Some("a".to_string())
    );
}

#[test]
fn transcode_only_sources_are_used_when_nothing_direct_plays() {
    let response = info(r#"{"MediaSources":[{"Id":"a"},{"Id":"b","TranscodingUrl":"/x.m3u8"}]}"#);
    assert_eq!(
        select_source(&response, None, None).and_then(|source| source.id.clone()),
        Some("b".to_string())
    );
}

#[test]
fn saved_preferences_map_to_play_options_including_subtitles_off() {
    let mut selected = options();
    apply_saved_preference(
        &mut selected,
        ResolvedPlaybackPreference {
            media_source_id: Some("source-b".to_string()),
            media_source_index: 1,
            audio_stream_index: Some(7),
            subtitle_stream_index: None,
        },
    );
    assert_eq!(selected.subtitle_stream_index, Some(-1));
}

#[test]
fn source_index_selects_a_source_without_a_stable_id() {
    let response = info(
        r#"{"MediaSources":[{"Name":"A","SupportsDirectStream":true},
                                {"Name":"B","SupportsDirectStream":true}]}"#,
    );
    assert_eq!(
        select_source(&response, None, Some(1)).map(MediaSourceInfo::display_name),
        Some("B")
    );
    assert_eq!(
        select_source(&response, Some("gone"), Some(1)).map(MediaSourceInfo::display_name),
        Some("A")
    );
}

#[test]
fn track_selection_prefers_the_server_default_then_the_first_audio_track() {
    let source = media_source(
        r#"{"Id":"src","DefaultAudioStreamIndex":3,"MediaStreams":[
                {"Index":1,"Type":"Video"},
                {"Index":2,"Type":"Audio","Language":"eng"},
                {"Index":3,"Type":"Audio","Language":"deu"}]}"#,
    );
    assert_eq!(
        choose_stream(&source, "Audio", None).map(|stream| stream.index),
        Some(3)
    );
    assert_eq!(
        choose_stream(&source, "Audio", Some(2)).map(|stream| stream.index),
        Some(2)
    );
    assert!(choose_stream(&source, "Audio", Some(99)).is_none());
    assert!(choose_stream(&source, "Audio", Some(-1)).is_none());

    let without_default = media_source(
        r#"{"Id":"src","MediaStreams":[{"Index":5,"Type":"Audio"},{"Index":6,"Type":"Audio"}]}"#,
    );
    assert_eq!(
        choose_stream(&without_default, "Audio", None).map(|stream| stream.index),
        Some(5)
    );
}

#[test]
fn subtitles_are_off_unless_the_server_or_the_user_picks_one() {
    let source = media_source(
        r#"{"Id":"src","MediaStreams":[{"Index":3,"Type":"Subtitle","Language":"eng"}]}"#,
    );
    assert!(choose_stream(&source, "Subtitle", None).is_none());
    assert_eq!(
        choose_stream(&source, "Subtitle", Some(3)).map(|stream| stream.index),
        Some(3)
    );
}

#[test]
fn explicit_subtitles_off_is_mapped_to_player_off_sentinels() {
    let response = info(
        r#"{"MediaSources":[{"Id":"src","Container":"mkv","SupportsDirectStream":true,
                "DefaultSubtitleStreamIndex":3,
                "MediaStreams":[{"Index":3,"Type":"Subtitle","Language":"eng"}]}]}"#,
    );
    let prepared = build(
        &client(),
        &response,
        StreamingQuality::Original,
        &PlayOptions {
            subtitle_stream_index: Some(-1),
            ..options()
        },
        0,
        None,
    )
    .expect("prepared");
    assert_eq!(prepared.request.subtitle_stream_index, Some(-1));
    assert_eq!(prepared.request.subtitle_mpv_id, Some(-1));
    assert_eq!(prepared.request.subtitle_url, None);
}

#[test]
fn embedded_ordinals_count_only_internal_tracks_of_the_same_type() {
    let source = media_source(
        r#"{"Id":"src","MediaStreams":[
                {"Index":0,"Type":"Video"},
                {"Index":1,"Type":"Audio"},
                {"Index":2,"Type":"Audio"},
                {"Index":3,"Type":"Subtitle","IsExternal":true},
                {"Index":4,"Type":"Subtitle"}]}"#,
    );
    assert_eq!(embedded_ordinal(&source, "Audio", 1), Some(1));
    assert_eq!(embedded_ordinal(&source, "Audio", 2), Some(2));
    assert_eq!(embedded_ordinal(&source, "Subtitle", 4), Some(1));
    assert_eq!(embedded_ordinal(&source, "Subtitle", 3), None);
}

#[test]
fn external_subtitles_are_handed_to_the_player_as_an_absolute_url() {
    let response = info(
        r#"{"MediaSources":[{"Id":"src","Container":"mkv","SupportsDirectStream":true,
                "MediaStreams":[{"Index":3,"Type":"Subtitle","IsExternal":true,
                                 "DeliveryUrl":"/Videos/item-1/subs.srt"}]}]}"#,
    );
    let prepared = build(
        &client(),
        &response,
        StreamingQuality::Original,
        &PlayOptions {
            subtitle_stream_index: Some(3),
            ..options()
        },
        0,
        None,
    )
    .expect("prepared");
    assert_eq!(
        prepared.request.subtitle_url.as_deref(),
        Some("http://server:8096/Videos/item-1/subs.srt")
    );
    assert_eq!(prepared.request.subtitle_stream_index, Some(3));
    assert_eq!(prepared.request.subtitle_mpv_id, None);
}

struct StubJellyfinServer {
    url: String,
    requests: std::sync::Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl StubJellyfinServer {
    fn start(handler: impl Fn(&str, usize) -> serde_json::Value + Send + Sync + 'static) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        listener.set_nonblocking(true).expect("nonblocking");
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let requests_ref = std::sync::Arc::clone(&requests);
        let stop_ref = std::sync::Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            use std::io::Write;
            let mut count = 0usize;
            while !stop_ref.load(std::sync::atomic::Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let req =
                            crate::jellyfin::playback_reporter::read_test_http_request(&mut stream);
                        let parsed_body = serde_json::from_str::<serde_json::Value>(&req.body)
                            .unwrap_or(serde_json::Value::Null);
                        let idx = count;
                        count += 1;
                        requests_ref
                            .lock()
                            .expect("lock requests")
                            .push((req.request_line.clone(), parsed_body));
                        let body_text = handler(&req.request_line, idx).to_string();
                        let reply = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body_text.len(),
                            body_text
                        );
                        let _ = stream.write_all(reply.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            handle: Some(handle),
        }
    }

    fn recorded(&self) -> Vec<(String, serde_json::Value)> {
        self.requests.lock().expect("lock").clone()
    }
}

impl Drop for StubJellyfinServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn prepare_direct_play_resolves_saved_and_language_preferences_in_one_playback_info_call() {
    let server = StubJellyfinServer::start(|line, _| {
        assert!(
            line.contains("/PlaybackInfo"),
            "expected single /PlaybackInfo call, got unexpected request: {line}"
        );
        serde_json::json!({
            "PlaySessionId": "ps-1",
            "MediaSources": [{
                "Id": "src-1",
                "Container": "mkv",
                "SupportsDirectPlay": true,
                "DefaultAudioStreamIndex": 1,
                "MediaStreams": [
                    { "Index": 0, "Type": "Video" },
                    { "Index": 1, "Type": "Audio", "Language": "eng" },
                    { "Index": 2, "Type": "Audio", "Language": "jpn" },
                    { "Index": 3, "Type": "Subtitle", "Language": "eng", "IsDefault": true }
                ]
            }]
        })
    });
    let fixture = crate::app::services::test_support::TestServices::signed_in(&server.url);
    let scope = fixture.services.session.scope().expect("scope");
    let saved = crate::library::ItemPlaybackPreference {
        media_source: crate::library::model::MediaSourcePreference {
            id: Some("src-1".to_string()),
            index: 0,
            name: "Default".to_string(),
            container: None,
        },
        audio_track: Some(crate::library::model::TrackPreference {
            index: 2,
            ..Default::default()
        }),
        subtitle_track: Some(crate::library::model::TrackPreference {
            index: 3,
            ..Default::default()
        }),
    };
    let mut opts = options();
    opts.viewing.audio_languages = vec!["en".into()];
    let prepared = super::prepare(
        &scope,
        &fixture.services.library,
        Some(&saved),
        StreamingQuality::Original,
        &opts,
    )
    .expect("prepare");
    let recorded = server.recorded();
    assert_eq!(recorded.len(), 1, "expected 1 HTTP call, got {recorded:?}");
    assert!(recorded[0].0.contains("/Items/item-1/PlaybackInfo"));
    assert_eq!(prepared.request.audio_stream_index, Some(2));
    assert_eq!(prepared.request.subtitle_stream_index, Some(3));
}

#[test]
fn prepare_transcode_requeries_playback_info_once_when_saved_preference_changes_tracks() {
    let server = StubJellyfinServer::start(|line, idx| {
        assert!(
            line.contains("/PlaybackInfo"),
            "expected /PlaybackInfo only, got unexpected request: {line}"
        );
        let url = if idx == 0 {
            "/videos/item-1/master.m3u8?AudioStreamIndex=1"
        } else {
            "/videos/item-1/master.m3u8?AudioStreamIndex=2&SubtitleStreamIndex=3"
        };
        serde_json::json!({
            "PlaySessionId": format!("ps-{idx}"),
            "MediaSources": [{
                "Id": "src-1",
                "SupportsDirectPlay": false,
                "SupportsDirectStream": false,
                "SupportsTranscoding": true,
                "TranscodingUrl": url,
                "MediaStreams": [
                    { "Index": 0, "Type": "Video" },
                    { "Index": 1, "Type": "Audio", "Language": "eng" },
                    { "Index": 2, "Type": "Audio", "Language": "jpn" },
                    { "Index": 3, "Type": "Subtitle", "Language": "eng" }
                ]
            }]
        })
    });
    let fixture = crate::app::services::test_support::TestServices::signed_in(&server.url);
    let scope = fixture.services.session.scope().expect("scope");
    let saved = crate::library::ItemPlaybackPreference {
        media_source: crate::library::model::MediaSourcePreference {
            id: Some("src-1".to_string()),
            index: 0,
            name: "Default".to_string(),
            container: None,
        },
        audio_track: Some(crate::library::model::TrackPreference {
            index: 2,
            ..Default::default()
        }),
        subtitle_track: Some(crate::library::model::TrackPreference {
            index: 3,
            ..Default::default()
        }),
    };
    let prepared = super::prepare(
        &scope,
        &fixture.services.library,
        Some(&saved),
        StreamingQuality::Mbps10,
        &options(),
    )
    .expect("prepare");
    let recorded = server.recorded();
    assert_eq!(
        recorded.len(),
        2,
        "expected 2 PlaybackInfo calls, got {recorded:?}"
    );
    assert_eq!(recorded[1].1["AudioStreamIndex"], 2);
    assert_eq!(prepared.request.audio_stream_index, Some(2));
    assert!(prepared.request.media_url.contains("AudioStreamIndex=2"));
}
