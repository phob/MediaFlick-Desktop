(() => {
  if (window.__jellyfinMpvBridgeInstalled) return;
  window.__jellyfinMpvBridgeInstalled = true;

  const contextsByItem = new Map();
  const contextsByMediaSource = new Map();
  const contextsByPlaySession = new Map();
  const sentContextKeys = new Map();
  const sentPlayKeys = new Map();
  const externalizedByItem = new Map();
  const externalizedByMediaSource = new Map();
  const externalizedByPlaySession = new Map();
  const externalizedMedia = new WeakMap();
  const externalizedSources = new WeakMap();
  const syntheticMediaState = new WeakMap();
  const syntheticMediaElements = new Set();
  const playerInstances = new Set();
  const CONTEXT_TTL_MS = 15 * 60 * 1000;
  const EXTERNALIZED_TTL_MS = 12 * 60 * 60 * 1000;
  const MPV_STOP_GRACE_MS = 2000;
  const MAX_BITRATE = 1000000000;
  const PLAYER_PLUGIN_NAME = 'jellyfinMpvPlayer';
  const nativeFetch = window.fetch;
  let playerStateTimer = 0;
  let playerStateFrame = null;
  let playerStateRequestId = 0;

  const MPV_DEVICE_PROFILE = {
    Name: 'jellyfin-mpv',
    MaxStaticBitrate: MAX_BITRATE,
    MaxStaticMusicBitrate: MAX_BITRATE,
    MaxStreamingBitrate: MAX_BITRATE,
    MusicStreamingTranscodingBitrate: 1280000,
    TimelineOffsetSeconds: 5,
    DirectPlayProfiles: [
      {
        Type: 'Video',
        Container: 'mkv,matroska,webm,mp4,m4v,mov,avi,mpg,mpeg,ts,m2ts,m2t,wmv,asf,flv,ogv,ogg,3gp,3g2,vob,iso,bluray,divx,xvid,rm,rmvb,nsv,wtv,dvr-ms,strm',
        VideoCodec: 'h264,hevc,h265,av1,mpeg4,mpeg2video,mpegvideo,vc1,vp8,vp9,wmv3,msmpeg4v2,msmpeg4v3,theora,mjpeg,dvvideo,prores,flv1,h263,h263p,rv40',
        AudioCodec: 'aac,ac3,eac3,truehd,dts,dca,flac,mp3,mp2,opus,vorbis,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le,alac,wma,wmav2,wmav1,ape,tta,wavpack,amr_nb,amr_wb,ra,mlp'
      },
      {
        Type: 'Audio',
        Container: 'mp3,flac,wav,m4a,aac,alac,ogg,oga,opus,wma,ape,aiff,aif,mka,dsf,dff,tta,wv',
        AudioCodec: 'aac,ac3,eac3,truehd,dts,dca,flac,mp3,mp2,opus,vorbis,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le,alac,wma,wmav2,wmav1,ape,tta,wavpack,amr_nb,amr_wb,ra,mlp'
      },
      { Type: 'Photo' }
    ],
    TranscodingProfiles: [],
    SubtitleProfiles: ['srt', 'subrip', 'ass', 'ssa', 'PGSSUB', 'DVDSUB', 'DVBSUB', 'DVBTXT', 'webvtt', 'vtt', 'microdvd', 'subviewer', 'subviewer1', 'sami', 'realtext', 'stl', 'ttml']
      .flatMap((Format) => [{ Format, Method: 'Embed' }, { Format, Method: 'External' }]),
    ResponseProfiles: [],
    ContainerProfiles: [],
    CodecProfiles: []
  };

  function absoluteUrl(value) {
    try { return new URL(String(value || ''), location.href).href; }
    catch (_) { return String(value || ''); }
  }

  function parsedUrl(value) {
    try { return new URL(absoluteUrl(value)); }
    catch (_) { return null; }
  }

  function itemIdFromPath(pathname) {
    const match = pathname.match(/\/(Videos|Audio)\/([^/?#]+)/i);
    return match ? decodeURIComponent(match[2]) : '';
  }

  function isPlaybackInfoUrl(value) {
    const url = parsedUrl(value);
    return !!url && /\/Items\/[^/?#]+\/PlaybackInfo/i.test(url.pathname);
  }

  function itemIdFromPlaybackInfoUrl(value) {
    const url = parsedUrl(value);
    if (!url) return '';
    const match = url.pathname.match(/\/Items\/([^/?#]+)\/PlaybackInfo/i);
    return match ? decodeURIComponent(match[1]) : '';
  }

  function isDirectStreamUrl(value) {
    const url = parsedUrl(value);
    if (!url) return false;
    const path = url.pathname.toLowerCase();
    if (!(path.includes('/videos/') || path.includes('/audio/'))) return false;
    if (path.includes('/hls') || path.includes('/dash') || path.includes('/transcoding')) return false;
    return path.includes('/stream') || path.includes('/original');
  }

  function streamContext(value) {
    const url = parsedUrl(value);
    if (!url) return { mediaUrl: absoluteUrl(value), title: document.title || '' };
    const context = {
      mediaUrl: url.href,
      itemId: itemIdFromPath(url.pathname),
      mediaSourceId: url.searchParams.get('MediaSourceId') || url.searchParams.get('mediaSourceId') || '',
      playSessionId: url.searchParams.get('PlaySessionId') || url.searchParams.get('playSessionId') || '',
      deviceId: url.searchParams.get('DeviceId') || url.searchParams.get('deviceId') || '',
      title: document.title || ''
    };
    const startTicks = url.searchParams.get('StartTimeTicks') || url.searchParams.get('startTimeTicks');
    if (startTicks && /^\d+$/.test(startTicks)) context.startTimeTicks = Number(startTicks);
    const runtimeTicks = url.searchParams.get('RuntimeTicks') || url.searchParams.get('runtimeTicks');
    if (runtimeTicks && /^\d+$/.test(runtimeTicks)) context.runtimeTicks = Number(runtimeTicks);
    return context;
  }

  function mergeContext(base, extra) {
    const out = Object.assign({}, base || {});
    for (const [key, value] of Object.entries(extra || {})) {
      if (value !== undefined && value !== null && value !== '') out[key] = value;
    }
    return out;
  }

  function remember(context) {
    if (!context) return;
    context.seenAt = Date.now();
    if (!context.title) context.title = document.title || '';
    if (context.itemId) contextsByItem.set(String(context.itemId), context);
    if (context.mediaSourceId) contextsByMediaSource.set(String(context.mediaSourceId), context);
    if (context.playSessionId) contextsByPlaySession.set(String(context.playSessionId), context);
    sendContext(context);
    pruneContexts();
  }

  function rememberForStream(value) {
    if (!isDirectStreamUrl(value)) return null;
    const context = streamContext(value);
    let merged = context;
    if (context.playSessionId && contextsByPlaySession.has(context.playSessionId)) {
      merged = mergeContext(contextsByPlaySession.get(context.playSessionId), merged);
    }
    if (context.mediaSourceId && contextsByMediaSource.has(context.mediaSourceId)) {
      merged = mergeContext(contextsByMediaSource.get(context.mediaSourceId), merged);
    }
    if (context.itemId && contextsByItem.has(context.itemId)) {
      merged = mergeContext(contextsByItem.get(context.itemId), merged);
    }
    remember(merged);
    markExternalized(merged);
    return merged;
  }

  function markExternalized(context) {
    if (!context) return;
    context.externalizedAt = Date.now();
    if (context.itemId) externalizedByItem.set(String(context.itemId), context);
    if (context.mediaSourceId) externalizedByMediaSource.set(String(context.mediaSourceId), context);
    if (context.playSessionId) externalizedByPlaySession.set(String(context.playSessionId), context);
    pruneExternalized();
  }

  function ticksToSeconds(value) {
    const number = numberish(value);
    return number && number > 0 ? number / 10000000 : 0;
  }

  function syntheticDuration(context) {
    return ticksToSeconds(context?.runtimeTicks);
  }

  function syntheticStartPosition(context) {
    return ticksToSeconds(context?.startTimeTicks);
  }

  function fakeTimeRanges(duration) {
    const end = Number.isFinite(duration) && duration > 0 ? duration : 0;
    return {
      length: end > 0 ? 1 : 0,
      start(index) {
        if (index !== 0 || end <= 0) throw new DOMException('Index out of range', 'IndexSizeError');
        return 0;
      },
      end(index) {
        if (index !== 0 || end <= 0) throw new DOMException('Index out of range', 'IndexSizeError');
        return end;
      }
    };
  }

  function dispatchMediaEvent(element, name) {
    try { element.dispatchEvent(new Event(name)); } catch (_) {}
  }

  function syntheticCurrentTime(state) {
    if (!state) return 0;
    let value = state.position || 0;
    if (!state.paused && !state.ended) {
      value += (Date.now() - state.updatedAt) / 1000;
    }
    if (state.duration > 0) value = Math.min(value, state.duration);
    return Math.max(0, value);
  }

  function setSyntheticCurrentTime(state, value) {
    const seconds = Math.max(0, Number(value) || 0);
    state.position = state.duration > 0 ? Math.min(seconds, state.duration) : seconds;
    state.updatedAt = Date.now();
    state.ended = false;
  }

  function clearSyntheticMediaState(element) {
    const state = syntheticMediaState.get(element);
    if (state?.timer) clearInterval(state.timer);
    syntheticMediaState.delete(element);
    syntheticMediaElements.delete(element);
  }

  function syntheticStateFor(element) {
    const state = syntheticMediaState.get(element);
    if (!state) return null;
    if (!contextWasExternalized(state.context)) {
      clearSyntheticMediaState(element);
      return null;
    }
    return state;
  }

  function ensureSyntheticMediaState(element, context) {
    if (!element || !context) return null;
    let state = syntheticMediaState.get(element);
    if (!state) {
      state = {
        context,
        position: syntheticStartPosition(context),
        duration: syntheticDuration(context),
        paused: true,
        ended: false,
        externalPlaybackActive: false,
        mpvObservedActive: false,
        mpvStartedAt: 0,
        updatedAt: Date.now(),
        timer: 0
      };
      syntheticMediaState.set(element, state);
      syntheticMediaElements.add(element);
    } else {
      state.context = mergeContext(state.context, context);
      const duration = syntheticDuration(state.context);
      if (duration > 0) state.duration = duration;
    }
    return state;
  }

  function signalSyntheticMediaReady(element) {
    setTimeout(() => {
      const state = syntheticStateFor(element);
      if (!state) return;
      dispatchMediaEvent(element, 'loadstart');
      dispatchMediaEvent(element, 'loadedmetadata');
      dispatchMediaEvent(element, 'durationchange');
      dispatchMediaEvent(element, 'loadeddata');
      dispatchMediaEvent(element, 'canplay');
      dispatchMediaEvent(element, 'canplaythrough');
    }, 0);
  }

  function startSyntheticMediaTimer(element, state) {
    if (!state || state.timer) return;
    state.timer = setInterval(() => {
      const current = syntheticStateFor(element);
      if (!current) return;
      current.context.externalizedAt = Date.now();
      if (current.paused || current.ended) return;
      const time = syntheticCurrentTime(current);
      dispatchMediaEvent(element, 'timeupdate');
      if (current.duration > 0 && time >= current.duration) {
        current.position = current.duration;
        current.updatedAt = Date.now();
        current.paused = true;
        current.ended = true;
        current.externalPlaybackActive = false;
        current.mpvObservedActive = false;
        clearInterval(current.timer);
        current.timer = 0;
        dispatchMediaEvent(element, 'ended');
        stopPlayerStatePollingIfIdle();
      }
    }, 1000);
  }

  function playSyntheticMedia(element) {
    return new Promise((resolve) => {
      setTimeout(() => {
        beginSyntheticPlayback(element);
        resolve();
      }, 0);
    });
  }

  function beginSyntheticPlayback(element) {
    const context = externalizedMedia.get(element)
      || streamContext(element.currentSrc || element.src || element.getAttribute?.('src') || '');
    const state = ensureSyntheticMediaState(element, context);
    if (!state) return;
    setSyntheticCurrentTime(state, syntheticCurrentTime(state));
    state.paused = false;
    state.ended = false;
    state.externalPlaybackActive = true;
    state.mpvObservedActive = false;
    state.mpvStartedAt = Date.now();
    state.context.externalizedAt = Date.now();
    dispatchMediaEvent(element, 'loadstart');
    dispatchMediaEvent(element, 'loadedmetadata');
    dispatchMediaEvent(element, 'durationchange');
    dispatchMediaEvent(element, 'loadeddata');
    dispatchMediaEvent(element, 'canplay');
    dispatchMediaEvent(element, 'canplaythrough');
    dispatchMediaEvent(element, 'play');
    dispatchMediaEvent(element, 'playing');
    dispatchMediaEvent(element, 'timeupdate');
    startSyntheticMediaTimer(element, state);
    ensurePlayerStatePolling();
  }

  function pauseSyntheticPlayback(element) {
    const state = syntheticStateFor(element);
    if (!state || state.paused) return;
    setSyntheticCurrentTime(state, syntheticCurrentTime(state));
    state.paused = true;
    dispatchMediaEvent(element, 'pause');
    dispatchMediaEvent(element, 'timeupdate');
  }

  function markMediaExternalized(element, context) {
    if (!element) return;
    if (context) {
      externalizedMedia.set(element, context);
      ensureSyntheticMediaState(element, context);
      signalSyntheticMediaReady(element);
    } else {
      externalizedMedia.delete(element);
      clearSyntheticMediaState(element);
    }
  }

  function markSourceExternalized(source, context) {
    if (typeof HTMLSourceElement === 'undefined' || !(source instanceof HTMLSourceElement)) return;
    if (context) {
      externalizedSources.set(source, context);
      const media = mediaElementForNode(source);
      if (media) markMediaExternalized(media, context);
    } else {
      externalizedSources.delete(source);
    }
  }

  function contextWasExternalized(context) {
    if (!context) return false;
    pruneExternalized();
    if (context.playSessionId && externalizedByPlaySession.has(String(context.playSessionId))) return true;
    if (context.mediaSourceId && externalizedByMediaSource.has(String(context.mediaSourceId))) return true;
    if (context.itemId && externalizedByItem.has(String(context.itemId))) return true;
    return false;
  }

  function mediaWasExternalized(element) {
    if (!element) return false;
    if (externalizedMedia.has(element)) {
      const context = externalizedMedia.get(element);
      if (contextWasExternalized(context)) {
        ensureSyntheticMediaState(element, context);
        return true;
      }
      externalizedMedia.delete(element);
      clearSyntheticMediaState(element);
    }
    const src = element.currentSrc || element.src || element.getAttribute?.('src') || '';
    if (isDirectStreamUrl(src)) {
      const context = streamContext(src);
      if (contextWasExternalized(context)) {
        externalizedMedia.set(element, context);
        ensureSyntheticMediaState(element, context);
        return true;
      }
    }
    for (const source of Array.from(element.querySelectorAll?.('source') || [])) {
      const stored = externalizedSources.get(source);
      if (stored && contextWasExternalized(stored)) {
        externalizedMedia.set(element, stored);
        ensureSyntheticMediaState(element, stored);
        return true;
      }
      const sourceUrl = source.src || source.getAttribute?.('src') || '';
      if (!isDirectStreamUrl(sourceUrl)) continue;
      const context = externalizedSources.get(source) || streamContext(sourceUrl);
      if (!contextWasExternalized(context)) continue;
      externalizedSources.set(source, context);
      externalizedMedia.set(element, context);
      ensureSyntheticMediaState(element, context);
      return true;
    }
    return false;
  }

  function rememberPlaybackInfo(requestUrl, data, requestBody) {
    if (!data || typeof data !== 'object') return;
    const itemId = itemIdFromPlaybackInfoUrl(requestUrl);
    let body = null;
    if (typeof requestBody === 'string' && requestBody.trim().startsWith('{')) {
      try { body = JSON.parse(requestBody); } catch (_) { body = null; }
    }
    const base = {
      itemId,
      playSessionId: data.PlaySessionId || '',
      deviceId: body?.DeviceId || '',
      startTimeTicks: numberish(body?.StartTimeTicks ?? body?.StartPositionTicks),
      audioStreamIndex: numberish(body?.AudioStreamIndex),
      subtitleStreamIndex: numberish(body?.SubtitleStreamIndex),
      playMethod: body?.PlayMethod || '',
      playlistItemId: body?.PlaylistItemId || '',
      queue: queueItems(body),
      details: body || undefined,
      title: document.title || ''
    };
    for (const source of (Array.isArray(data.MediaSources) ? data.MediaSources : [])) {
      const audioStreamIndex = numberish(base.audioStreamIndex ?? source.DefaultAudioStreamIndex);
      const subtitleStreamIndex = numberish(base.subtitleStreamIndex ?? source.DefaultSubtitleStreamIndex);
      const context = mergeContext(base, mergeContext({
        mediaSourceId: source.Id || source.MediaSourceId || '',
        runtimeTicks: numberish(source.RunTimeTicks),
        startTimeTicks: numberish(base.startTimeTicks ?? source.StartTimeTicks),
        audioStreamIndex,
        subtitleStreamIndex,
        title: source.Name || source.Path || base.title
      }, selectedTrackContext(source, audioStreamIndex, subtitleStreamIndex)));
      remember(context);
    }
    if (!data.MediaSources || !data.MediaSources.length) remember(base);
  }

  function numberish(value) {
    if (value === undefined || value === null || value === '') return undefined;
    const number = Number(value);
    return Number.isFinite(number) ? number : undefined;
  }

  function queueItems(value) {
    if (!value || typeof value !== 'object') return undefined;
    if (Array.isArray(value.NowPlayingQueue)) return value.NowPlayingQueue;
    if (Array.isArray(value.Queue)) return value.Queue;
    return undefined;
  }

  function streamType(stream) {
    return String(stream?.Type || stream?.type || '').toLowerCase();
  }

  function streamIndex(stream) {
    return numberish(stream?.Index ?? stream?.index);
  }

  function isExternalStream(stream) {
    return stream?.IsExternal === true || stream?.isExternal === true;
  }

  function deliveryMethod(stream) {
    return String(stream?.DeliveryMethod || stream?.deliveryMethod || '').toLowerCase();
  }

  function deliveryUrl(stream) {
    return stream?.DeliveryUrl || stream?.deliveryUrl || '';
  }

  function mediaStreams(mediaSource) {
    if (Array.isArray(mediaSource?.MediaStreams)) return mediaSource.MediaStreams;
    if (Array.isArray(mediaSource?.mediaStreams)) return mediaSource.mediaStreams;
    return [];
  }

  function selectedAudioMpvId(mediaSource, selectedIndex) {
    const wanted = numberish(selectedIndex);
    if (wanted === undefined) return undefined;
    let mpvId = 1;
    for (const stream of mediaStreams(mediaSource)) {
      if (streamType(stream) !== 'audio') continue;
      if (streamIndex(stream) === wanted) return mpvId;
      if (!isExternalStream(stream)) mpvId += 1;
    }
    return undefined;
  }

  function selectedSubtitleTrack(mediaSource, selectedIndex) {
    const wanted = numberish(selectedIndex);
    if (wanted === undefined) return {};
    if (wanted < 0) return { subtitleMpvId: -1 };

    let mpvId = 1;
    for (const stream of mediaStreams(mediaSource)) {
      if (streamType(stream) !== 'subtitle') continue;
      if (streamIndex(stream) === wanted) {
        const method = deliveryMethod(stream);
        const url = deliveryUrl(stream);
        if (method === 'external' && url) return { subtitleUrl: absoluteUrl(url) };
        if (method === 'embed' || (!method && !isExternalStream(stream))) return { subtitleMpvId: mpvId };
        return {};
      }
      if (!isExternalStream(stream)) mpvId += 1;
    }
    return {};
  }

  function selectedTrackContext(mediaSource, audioIndex, subtitleIndex) {
    const out = {};
    const audioMpvId = selectedAudioMpvId(mediaSource, audioIndex);
    if (audioMpvId !== undefined) out.audioMpvId = audioMpvId;
    Object.assign(out, selectedSubtitleTrack(mediaSource, subtitleIndex));
    return out;
  }

  function cloneMpvDeviceProfile() {
    return JSON.parse(JSON.stringify(MPV_DEVICE_PROFILE));
  }

  function patchPlaybackInfoBody(requestUrl, body) {
    if (!isPlaybackInfoUrl(requestUrl) || typeof body !== 'string' || !body.trim().startsWith('{')) {
      return body;
    }

    try {
      const dto = JSON.parse(body);
      dto.DeviceProfile = cloneMpvDeviceProfile();
      dto.MaxStreamingBitrate = Math.max(numberish(dto.MaxStreamingBitrate) || 0, MAX_BITRATE);
      dto.EnableDirectPlay = true;
      dto.EnableDirectStream = true;
      dto.AllowVideoStreamCopy = true;
      dto.AllowAudioStreamCopy = true;
      dto.AutoOpenLiveStream = true;
      // Direct-play handoff only: if the browser asks for transcoding first,
      // Jellyfin may never emit a raw /Videos|Audio/.../stream URL for us to capture.
      dto.EnableTranscoding = false;
      return JSON.stringify(dto);
    } catch (error) {
      console.debug('[jellyfin-mpv] failed to patch PlaybackInfo profile', error);
      return body;
    }
  }

  function sendContext(context) {
    try {
      const clean = Object.assign({}, context);
      delete clean.seenAt;
      const key = [clean.mediaUrl || '', clean.playSessionId || '', clean.mediaSourceId || '', clean.itemId || '', clean.startTimeTicks || ''].join('|');
      const last = sentContextKeys.get(key) || 0;
      if (Date.now() - last < 1000) return;
      sentContextKeys.set(key, Date.now());
      sendBridgeRequest('play-context', clean);
    } catch (error) {
      console.debug('[jellyfin-mpv] bridge context send failed', error);
    }
  }

  function sendExternalPlayback(context) {
    if (!context || !context.mediaUrl) return;
    try {
      const clean = Object.assign({}, context);
      delete clean.seenAt;
      delete clean.externalizedAt;
      const key = [clean.mediaUrl || '', clean.playSessionId || '', clean.mediaSourceId || '', clean.itemId || '', clean.startTimeTicks || ''].join('|');
      const last = sentPlayKeys.get(key) || 0;
      if (Date.now() - last < 1000) return;
      sentPlayKeys.set(key, Date.now());
      sendBridgeRequest('play', clean);
    } catch (error) {
      console.debug('[jellyfin-mpv] bridge play send failed', error);
    }
  }

  function sendPlayerCommand(payload) {
    try {
      sendBridgeRequest('player-command', payload);
    } catch (error) {
      console.debug('[jellyfin-mpv] bridge player command send failed', error);
    }
  }

  function activePlayerCount() {
    let count = 0;
    for (const player of playerInstances) {
      if (player?._currentSrc) count += 1;
    }
    for (const element of Array.from(syntheticMediaElements)) {
      const state = syntheticStateFor(element);
      if (state?.externalPlaybackActive) count += 1;
    }
    return count;
  }

  function applyMpvSnapshotToSyntheticMedia(snapshot) {
    if (!snapshot) return;
    for (const element of Array.from(syntheticMediaElements)) {
      const state = syntheticStateFor(element);
      if (!state?.externalPlaybackActive) continue;

      if (snapshot.active === true) {
        state.mpvObservedActive = true;
      } else if (snapshot.active === false && shouldAcceptMpvStop(state)) {
        finishSyntheticPlaybackFromMpv(element, state, snapshot);
        continue;
      }

      const position = Number(snapshot.positionMs);
      if (Number.isFinite(position) && position >= 0) {
        const seconds = position / 1000;
        const previous = syntheticCurrentTime(state);
        setSyntheticCurrentTime(state, seconds);
        if (Math.abs(previous - seconds) > 0.25) {
          dispatchMediaEvent(element, 'timeupdate');
        }
      }

      const duration = Number(snapshot.durationMs);
      if (Number.isFinite(duration) && duration > 0) {
        state.duration = duration / 1000;
      }

      if (typeof snapshot.paused === 'boolean' && snapshot.paused !== state.paused) {
        setSyntheticCurrentTime(state, syntheticCurrentTime(state));
        state.paused = snapshot.paused;
        dispatchMediaEvent(element, snapshot.paused ? 'pause' : 'playing');
      }
    }
  }

  function shouldAcceptMpvStop(state) {
    return state?.mpvObservedActive === true
      || (state?.mpvStartedAt && Date.now() - state.mpvStartedAt >= MPV_STOP_GRACE_MS);
  }

  function mpvStopReason(snapshot) {
    return String(snapshot?.stopReason || snapshot?.reason || '').trim().toLowerCase();
  }

  function shouldTreatMpvStopAsEnded(snapshot) {
    const reason = mpvStopReason(snapshot);
    return reason === 'eof' || reason === 'watched-next';
  }

  function suppressNextPlaybackAfterManualMpvStop(playbackManager, snapshot) {
    if (shouldTreatMpvStopAsEnded(snapshot)) return;
    if (playbackManager && typeof playbackManager === 'object') {
      playbackManager._playNextAfterEnded = false;
    }
  }

  function finishSyntheticPlaybackFromMpv(element, state, snapshot) {
    const position = Number(snapshot?.positionMs);
    if (Number.isFinite(position) && position >= 0) {
      setSyntheticCurrentTime(state, position / 1000);
    } else {
      setSyntheticCurrentTime(state, syntheticCurrentTime(state));
    }
    if (state.timer) clearInterval(state.timer);
    state.timer = 0;
    const ended = shouldTreatMpvStopAsEnded(snapshot);
    state.paused = true;
    state.ended = ended;
    state.externalPlaybackActive = false;
    state.mpvObservedActive = false;
    state.mpvStartedAt = 0;
    dispatchMediaEvent(element, 'pause');
    dispatchMediaEvent(element, 'timeupdate');
    if (ended) dispatchMediaEvent(element, 'ended');
    stopPlayerStatePollingIfIdle();
  }

  function stopSyntheticMediaFromMpv(snapshot) {
    let handled = 0;
    for (const element of Array.from(syntheticMediaElements)) {
      const state = syntheticStateFor(element);
      if (state?.externalPlaybackActive) {
        finishSyntheticPlaybackFromMpv(element, state, snapshot);
        handled += 1;
      }
    }
    return handled;
  }

  function ensurePlayerStatePolling() {
    if (!playerStateTimer) {
      playerStateTimer = setInterval(requestPlayerState, 750);
    }
    requestPlayerState();
  }

  function stopPlayerStatePollingIfIdle() {
    if (activePlayerCount() > 0) return;
    if (playerStateTimer) clearInterval(playerStateTimer);
    playerStateTimer = 0;
    if (playerStateFrame?.parentNode) playerStateFrame.parentNode.removeChild(playerStateFrame);
    playerStateFrame = null;
  }

  function requestPlayerState() {
    if (activePlayerCount() === 0) {
      stopPlayerStatePollingIfIdle();
      return;
    }
    try {
      if (!playerStateFrame) {
        playerStateFrame = document.createElement('iframe');
        playerStateFrame.tabIndex = -1;
        playerStateFrame.setAttribute('aria-hidden', 'true');
        playerStateFrame.style.cssText = 'position:absolute;width:0;height:0;border:0;opacity:0;pointer-events:none;';
        (document.body || document.documentElement).appendChild(playerStateFrame);
      }
      playerStateRequestId += 1;
      playerStateFrame.src = 'jellyfin-mpv://player-state?requestId='
        + encodeURIComponent(String(playerStateRequestId))
        + '&t='
        + Date.now();
    } catch (error) {
      console.debug('[jellyfin-mpv] player state request failed', error);
    }
  }

  window.__jellyfinMpvReceivePlayerState = function(snapshot) {
    for (const player of Array.from(playerInstances)) {
      try {
        player._applyMpvSnapshot(snapshot);
      } catch (error) {
        console.debug('[jellyfin-mpv] failed to apply mpv state snapshot', error);
      }
    }
    try {
      applyMpvSnapshotToSyntheticMedia(snapshot);
    } catch (error) {
      console.debug('[jellyfin-mpv] failed to apply mpv state to synthetic media', error);
    }
  };

  window.__jellyfinMpvPlaybackStopped = function(snapshot) {
    let handledPlayers = 0;
    let handledSynthetic = 0;
    for (const player of Array.from(playerInstances)) {
      try {
        if (player._handleMpvStopped(snapshot)) handledPlayers += 1;
      } catch (error) {
        console.debug('[jellyfin-mpv] failed to handle mpv stopped event', error);
      }
    }
    try {
      if (handledPlayers === 0) {
        handledSynthetic = stopSyntheticMediaFromMpv(snapshot);
      }
    } catch (error) {
      console.debug('[jellyfin-mpv] failed to stop synthetic media after mpv stopped', error);
    }
    sendBridgeRequest('playback-stop-ack', {
      active: snapshot?.active,
      positionMs: snapshot?.positionMs,
      stopReason: snapshot?.stopReason,
      handledPlayers,
      handledSynthetic,
      activePlayers: activePlayerCount()
    });
  };

  function sendBridgeRequest(action, payload) {
    const url = 'jellyfin-mpv://' + action + '?payload=' + encodeURIComponent(JSON.stringify(payload));
    if (typeof nativeFetch === 'function') {
      try {
        nativeFetch.call(window, url, {
          method: 'GET',
          mode: 'no-cors',
          cache: 'no-store',
          credentials: 'omit',
          keepalive: true
        }).catch(() => {});
      } catch (_) {}
    }
    const image = new Image();
    image.src = url;
    setTimeout(() => {
      image.src = '';
    }, 1000);
  }

  function pruneContexts() {
    const cutoff = Date.now() - CONTEXT_TTL_MS;
    for (const map of [contextsByItem, contextsByMediaSource, contextsByPlaySession]) {
      for (const [key, context] of map.entries()) {
        if ((context.seenAt || 0) < cutoff) map.delete(key);
      }
    }
  }

  function pruneExternalized() {
    const cutoff = Date.now() - EXTERNALIZED_TTL_MS;
    for (const map of [externalizedByItem, externalizedByMediaSource, externalizedByPlaySession]) {
      for (const [key, context] of map.entries()) {
        if ((context.externalizedAt || 0) < cutoff) map.delete(key);
      }
    }
  }

  function isPlaystateReportUrl(value) {
    const url = parsedUrl(value);
    if (!url) return false;
    const path = url.pathname.toLowerCase();
    return /\/sessions\/playing(?:\/progress|\/stopped)?$/i.test(path)
      || /\/playingitems\/[^/]+(?:\/progress)?$/i.test(path);
  }

  function playstateContext(requestUrl, body) {
    const url = parsedUrl(requestUrl);
    if (!url) return null;
    let parsedBody = null;
    if (typeof body === 'string' && body.trim().startsWith('{')) {
      try { parsedBody = JSON.parse(body); } catch (_) { parsedBody = null; }
    }
    const context = {
      itemId: parsedBody?.ItemId || parsedBody?.itemId || '',
      mediaSourceId: parsedBody?.MediaSourceId || parsedBody?.mediaSourceId || url.searchParams.get('mediaSourceId') || '',
      playSessionId: parsedBody?.PlaySessionId || parsedBody?.playSessionId || url.searchParams.get('playSessionId') || '',
      audioStreamIndex: numberish(parsedBody?.AudioStreamIndex ?? parsedBody?.audioStreamIndex),
      subtitleStreamIndex: numberish(parsedBody?.SubtitleStreamIndex ?? parsedBody?.subtitleStreamIndex),
      playMethod: parsedBody?.PlayMethod || parsedBody?.playMethod || '',
      playlistItemId: parsedBody?.PlaylistItemId || parsedBody?.playlistItemId || '',
      queue: queueItems(parsedBody) || (Array.isArray(parsedBody?.queue) ? parsedBody.queue : undefined),
      details: parsedBody || undefined
    };
    if (!context.itemId) {
      const match = url.pathname.match(/\/PlayingItems\/([^/?#]+)/i);
      if (match) context.itemId = decodeURIComponent(match[1]);
    }
    return context;
  }

  function shouldSuppressPlaystateReport(requestUrl, body) {
    return isPlaystateReportUrl(requestUrl) && contextWasExternalized(playstateContext(requestUrl, body));
  }

  function syntheticNoContentResponse(requestUrl) {
    return new Response(null, {
      status: 204,
      statusText: 'No Content',
      headers: { 'X-Jellyfin-Mpv': 'externalized' }
    });
  }

  function completeSyntheticXhr(xhr, requestUrl) {
    try {
      Object.defineProperty(xhr, 'readyState', { configurable: true, value: 4 });
      Object.defineProperty(xhr, 'status', { configurable: true, value: 204 });
      Object.defineProperty(xhr, 'statusText', { configurable: true, value: 'No Content' });
      Object.defineProperty(xhr, 'responseURL', { configurable: true, value: requestUrl || location.href });
      Object.defineProperty(xhr, 'responseText', { configurable: true, value: '' });
      Object.defineProperty(xhr, 'response', { configurable: true, value: '' });
    } catch (_) {}
    setTimeout(() => {
      xhr.dispatchEvent(new Event('readystatechange'));
      xhr.dispatchEvent(new ProgressEvent('load'));
      xhr.dispatchEvent(new ProgressEvent('loadend'));
    }, 0);
  }

  function mediaElementForNode(node) {
    if (node instanceof HTMLMediaElement) return node;
    if (typeof HTMLSourceElement !== 'undefined' && node instanceof HTMLSourceElement) {
      return node.parentElement instanceof HTMLMediaElement ? node.parentElement : null;
    }
    return null;
  }

  function externalizedContextForNode(node) {
    if (!node) return null;
    let context = null;
    if (node instanceof HTMLMediaElement) {
      context = externalizedMedia.get(node) || null;
    } else if (typeof HTMLSourceElement !== 'undefined' && node instanceof HTMLSourceElement) {
      context = externalizedSources.get(node) || null;
    }
    if (!context) return null;
    if (contextWasExternalized(context)) return context;
    if (node instanceof HTMLMediaElement) {
      externalizedMedia.delete(node);
      clearSyntheticMediaState(node);
    } else if (typeof HTMLSourceElement !== 'undefined' && node instanceof HTMLSourceElement) {
      externalizedSources.delete(node);
    }
    return null;
  }

  function canPlayExternally(type) {
    const value = String(type || '').toLowerCase();
    if (!value) return false;
    return /^(audio|video)\//.test(value)
      || value.includes('matroska')
      || value.includes('mkv')
      || value.includes('ogg')
      || value.includes('mpegurl')
      || value.includes('dash+xml')
      || value.includes('octet-stream');
  }

  function ticksToMilliseconds(value) {
    const number = numberish(value);
    return number && number > 0 ? Math.round(number / 10000) : null;
  }

  function cssUrl(value) {
    return String(value || '').replace(/\\/g, '\\\\').replace(/'/g, "\\'");
  }

  function contextFromPlayOptions(options) {
    const rawUrl = options?.url || '';
    const mediaUrl = rawUrl ? absoluteUrl(rawUrl) : '';
    const mediaSource = options?.mediaSource || {};
    const item = options?.item || {};
    const startTimeTicks = numberish(
      options?.playerStartPositionTicks
        ?? options?.startTimeTicks
        ?? options?.StartTimeTicks
        ?? mediaSource.StartTimeTicks
    );
    const context = mediaUrl && isDirectStreamUrl(mediaUrl)
      ? streamContext(mediaUrl)
      : { mediaUrl, title: document.title || '' };

    const audioStreamIndex = numberish(
      options?.audioStreamIndex
        ?? options?.AudioStreamIndex
        ?? mediaSource.DefaultAudioStreamIndex
        ?? mediaSource.defaultAudioStreamIndex
    );
    const subtitleStreamIndex = numberish(
      options?.subtitleStreamIndex
        ?? options?.SubtitleStreamIndex
        ?? mediaSource.DefaultSubtitleStreamIndex
        ?? mediaSource.defaultSubtitleStreamIndex
    );

    return mergeContext(context, mergeContext({
      mediaUrl,
      itemId: item.Id || options?.itemId || context.itemId || '',
      mediaSourceId: mediaSource.Id || mediaSource.MediaSourceId || options?.mediaSourceId || context.mediaSourceId || '',
      playSessionId: options?.playSessionId || options?.PlaySessionId || mediaSource.PlaySessionId || context.playSessionId || '',
      startTimeTicks,
      runtimeTicks: numberish(mediaSource.RunTimeTicks ?? item.RunTimeTicks ?? context.runtimeTicks),
      audioStreamIndex,
      subtitleStreamIndex,
      playMethod: options?.playMethod || context.playMethod || '',
      playlistItemId: options?.playlistItemId || options?.PlaylistItemId || context.playlistItemId || '',
      queue: queueItems(options) || context.queue,
      details: options || context.details,
      title: item.Name || mediaSource.Name || document.title || context.title || ''
    }, selectedTrackContext(mediaSource, audioStreamIndex, subtitleStreamIndex)));
  }

  class JellyfinMpvPlayer {
    constructor(args = {}) {
      this.events = args.events;
      this.appHost = args.appHost;
      this.appSettings = args.appSettings;
      this.loading = args.loading;
      this.appRouter = args.appRouter;
      this.globalize = args.globalize;
      this.playbackManager = args.playbackManager;

      this.name = 'Jellyfin MPV Player';
      this.type = 'mediaplayer';
      this.id = 'jellyfinmpvplayer';
      this.priority = -1;
      this.syncPlayWrapAs = 'htmlvideoplayer';
      this.useFullSubtitleUrls = true;
      this.isLocalPlayer = true;
      this.isFetching = false;

      this._currentSrc = null;
      this._currentPlayOptions = null;
      this._currentTime = null;
      this._duration = null;
      this._timeBaseMs = 0;
      this._timeBaseAt = 0;
      this._paused = false;
      this._muted = false;
      this._volume = this._readSavedVolume();
      this._playRate = 1;
      this._timer = 0;
      this._videoContainer = null;
      this._mpvObservedActive = false;
      this._mpvStartedAt = 0;
      this._currentSubtitleOffset = 0;
      this._showSubtitleOffset = false;
      this._currentAspectRatio = 'auto';

      playerInstances.add(this);
    }

    _readSavedVolume() {
      try {
        const saved = Number(this.appSettings?.get?.('volume'));
        return Number.isFinite(saved) && saved >= 0 ? Math.round(saved * 100) : 100;
      } catch (_) {
        return 100;
      }
    }

    _saveVolume(value) {
      try {
        this.appSettings?.set?.('volume', (value || 100) / 100);
      } catch (_) {}
    }

    _trigger(name, detail) {
      try {
        if (detail === undefined) {
          this.events?.trigger?.(this, name);
        } else {
          this.events?.trigger?.(this, name, detail);
        }
      } catch (error) {
        console.debug('[jellyfin-mpv] failed to trigger player event', name, error);
      }
    }

    _isVideoOptions(options = this._currentPlayOptions) {
      return String(options?.item?.MediaType || options?.mediaType || '').toLowerCase() !== 'audio';
    }

    _sampleCurrentTime() {
      let value = this._currentTime || 0;
      if (!this._paused && this._timeBaseAt) {
        value = this._timeBaseMs + (Date.now() - this._timeBaseAt) * (this._playRate || 1);
      }
      if (this._duration && this._duration > 0) value = Math.min(value, this._duration);
      return Math.max(0, value);
    }

    _commitCurrentTime() {
      this._currentTime = this._sampleCurrentTime();
      this._timeBaseMs = this._currentTime;
      this._timeBaseAt = Date.now();
      return this._currentTime;
    }

    _startSyntheticClock() {
      this._stopSyntheticClock();
      this._timer = setInterval(() => {
        if (this._paused || !this._currentSrc) return;
        this._commitCurrentTime();
        this._trigger('timeupdate');
        if (this._duration && this._currentTime >= this._duration) {
          this.stop(false);
        }
      }, 1000);
    }

    _stopSyntheticClock() {
      if (this._timer) clearInterval(this._timer);
      this._timer = 0;
    }

    _createVideoContainer(options) {
      if (!this._isVideoOptions(options)) return;
      let container = document.querySelector('.videoPlayerContainer[data-jellyfin-mpv="true"]');
      if (!container) {
        container = document.createElement('div');
        container.className = 'videoPlayerContainer';
        container.dataset.jellyfinMpv = 'true';
        container.style.cssText = [
          'position:fixed',
          'top:0',
          'right:0',
          'bottom:0',
          'left:0',
          'display:flex',
          'align-items:center',
          'justify-content:center',
          'background:#000',
          'z-index:1000'
        ].join(';');
        document.body.insertBefore(container, document.body.firstChild);
      }

      if (options?.fullscreen) {
        container.classList.add('videoPlayerContainer-onTop');
        container.style.zIndex = '1000';
      }
      const background = options?.backdropUrl
        ? "#000 url('" + cssUrl(options.backdropUrl) + "') center/cover no-repeat"
        : '#000';
      container.style.background = background;
      if (options?.fullscreen) document.body.classList.add('hide-scroll');
      this._videoContainer = container;
    }

    _removeVideoContainer() {
      document.body.classList.remove('hide-scroll');
      const container = this._videoContainer;
      this._videoContainer = null;
      if (container?.dataset?.jellyfinMpv === 'true' && container.parentNode) {
        container.parentNode.removeChild(container);
      }
    }

    _notifyPlaying(options) {
      this.loading?.hide?.();
      if (this._videoContainer) {
        this._videoContainer.classList.remove('videoPlayerContainer-onTop');
        this._videoContainer.style.zIndex = 'unset';
      }
      this._trigger('unpause');
      this._trigger('playing');
      this._trigger('timeupdate');
      if (this._isVideoOptions(options) && this.appRouter?.showVideoOsd) {
        Promise.resolve(this.appRouter.showVideoOsd()).catch(() => {});
      }
    }

    _applyMpvSnapshot(snapshot) {
      if (!snapshot || !this._currentSrc) return;
      if (snapshot.active === true) {
        this._mpvObservedActive = true;
      } else if (snapshot.active === false && this._shouldAcceptMpvStop()) {
        this._handleMpvStopped(snapshot);
        return;
      }

      const position = Number(snapshot.positionMs);
      if (Number.isFinite(position) && position >= 0) {
        const previous = this._sampleCurrentTime();
        this._currentTime = position;
        this._timeBaseMs = position;
        this._timeBaseAt = Date.now();
        if (Math.abs(previous - position) > 250) {
          this._trigger('timeupdate');
        }
      }

      const duration = Number(snapshot.durationMs);
      if (Number.isFinite(duration) && duration > 0) {
        this._duration = duration;
      }

      if (typeof snapshot.paused === 'boolean' && snapshot.paused !== this._paused) {
        this._paused = snapshot.paused;
        this._timeBaseMs = this._currentTime || 0;
        this._timeBaseAt = Date.now();
        this._trigger(snapshot.paused ? 'pause' : 'unpause');
        if (!snapshot.paused) this._trigger('playing');
      }

      const volume = Number(snapshot.volume);
      let volumeChanged = false;
      if (Number.isFinite(volume) && volume !== this._volume) {
        this._volume = Math.max(0, Math.min(100, volume));
        volumeChanged = true;
      }
      if (typeof snapshot.mute === 'boolean' && snapshot.mute !== this._muted) {
        this._muted = snapshot.mute;
        volumeChanged = true;
      }
      if (volumeChanged) this._trigger('volumechange');
    }

    _shouldAcceptMpvStop() {
      return this._mpvObservedActive === true
        || (this._mpvStartedAt && Date.now() - this._mpvStartedAt >= MPV_STOP_GRACE_MS);
    }

    _handleMpvStopped(snapshot) {
      if (!this._currentSrc) return false;
      const position = Number(snapshot?.positionMs);
      if (Number.isFinite(position) && position >= 0) {
        this._currentTime = position;
        this._timeBaseMs = position;
        this._timeBaseAt = Date.now();
      }
      suppressNextPlaybackAfterManualMpvStop(this.playbackManager, snapshot);
      this._finishPlayback(false, false);
      return true;
    }

    play(options = {}) {
      console.debug('[jellyfin-mpv] external player play()', options);
      playerInstances.add(this);
      this._stopSyntheticClock();
      this._currentPlayOptions = options;

      const context = contextFromPlayOptions(options);
      remember(context);
      markExternalized(context);
      sendExternalPlayback(context);

      this._currentSrc = context.mediaUrl || absoluteUrl(options.url || '');
      this._mpvObservedActive = false;
      this._mpvStartedAt = Date.now();
      this._duration = ticksToMilliseconds(context.runtimeTicks);
      this._currentTime = ticksToMilliseconds(context.startTimeTicks) || 0;
      this._timeBaseMs = this._currentTime;
      this._timeBaseAt = Date.now();
      this._paused = false;

      if (this._isVideoOptions(options)) {
        this.loading?.show?.();
        this._createVideoContainer(options);
      }

      this._startSyntheticClock();
      ensurePlayerStatePolling();
      setTimeout(() => this._notifyPlaying(options), 0);
      return Promise.resolve();
    }

    _finishPlayback(destroyPlayer, notifyMpv) {
      const previousSrc = this._currentSrc;
      this._stopSyntheticClock();
      this._currentSrc = null;
      this._currentPlayOptions = null;
      this._currentTime = null;
      this._duration = null;
      this._timeBaseMs = 0;
      this._timeBaseAt = 0;
      this._paused = false;
      this._mpvObservedActive = false;
      this._mpvStartedAt = 0;
      if (previousSrc && notifyMpv) sendPlayerCommand({ command: 'stop' });
      if (previousSrc) this._trigger('stopped', [{ src: previousSrc }]);
      if (destroyPlayer) this.destroy();
      stopPlayerStatePollingIfIdle();
      return Promise.resolve();
    }

    stop(destroyPlayer) {
      return this._finishPlayback(destroyPlayer, true);
    }

    destroy() {
      this._stopSyntheticClock();
      this._removeVideoContainer();
      this._mpvObservedActive = false;
      this._mpvStartedAt = 0;
      playerInstances.delete(this);
      stopPlayerStatePollingIfIdle();
    }

    currentSrc() {
      return this._currentSrc;
    }

    getDeviceProfile(item, options) {
      if (this.appHost?.getDeviceProfile) {
        return Promise.resolve(this.appHost.getDeviceProfile(item, options));
      }
      return Promise.resolve(cloneMpvDeviceProfile());
    }

    canPlayMediaType(mediaType) {
      const value = String(mediaType || '').toLowerCase();
      return value === 'video' || value === 'audio';
    }

    canPlayItem(item) {
      return this.canPlayMediaType(item?.MediaType);
    }

    supportsPlayMethod() {
      return true;
    }

    supports(feature) {
      return ['PlaybackRate', 'SetAspectRatio'].includes(feature);
    }

    pause() {
      if (this._paused) return;
      this._commitCurrentTime();
      this._paused = true;
      sendPlayerCommand({ command: 'set-pause', pause: true });
      this._trigger('pause');
    }

    resume() {
      this.unpause();
    }

    unpause() {
      if (!this._paused) return;
      this._paused = false;
      this._timeBaseMs = this._currentTime || 0;
      this._timeBaseAt = Date.now();
      sendPlayerCommand({ command: 'set-pause', pause: false });
      this._trigger('unpause');
      this._trigger('playing');
    }

    paused() {
      return this._paused;
    }

    currentTime(value) {
      if (value != null) {
        const next = Math.max(0, Number(value) || 0);
        this._currentTime = this._duration && this._duration > 0 ? Math.min(next, this._duration) : next;
        this._timeBaseMs = this._currentTime;
        this._timeBaseAt = Date.now();
        sendPlayerCommand({ command: 'seek', positionMs: this._currentTime });
        this._trigger('timeupdate');
        return;
      }
      return this._sampleCurrentTime();
    }

    currentTimeAsync() {
      return Promise.resolve(this.currentTime());
    }

    duration() {
      return this._duration || null;
    }

    seekable() {
      return Boolean(this._duration);
    }

    getBufferedRanges() {
      return [];
    }

    setPlaybackRate(value) {
      this._commitCurrentTime();
      const rate = Number(value);
      this._playRate = Number.isFinite(rate) && rate > 0 ? rate : 1;
      sendPlayerCommand({ command: 'set-playback-rate', rate: this._playRate });
    }

    getPlaybackRate() {
      return this._playRate || 1;
    }

    getSupportedPlaybackRates() {
      return [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 2.5, 3, 3.5, 4]
        .map((id) => ({ id, name: id + 'x' }));
    }

    setVolume(value, save = true) {
      const next = Number(value);
      if (!Number.isFinite(next)) return;
      this._volume = Math.max(0, Math.min(100, next));
      if (save) this._saveVolume(this._volume);
      sendPlayerCommand({ command: 'set-volume', volume: this._volume });
      this._trigger('volumechange');
    }

    getVolume() {
      return this._volume;
    }

    volumeUp() {
      this.setVolume(Math.min(this._volume + 2, 100));
    }

    volumeDown() {
      this.setVolume(Math.max(this._volume - 2, 0));
    }

    setMute(mute, triggerEvent = true) {
      this._muted = Boolean(mute);
      sendPlayerCommand({ command: 'set-mute', mute: this._muted });
      if (triggerEvent) this._trigger('volumechange');
    }

    isMuted() {
      return this._muted;
    }

    setAudioStreamIndex(index) {
      const value = numberish(index);
      const mediaSource = this._currentPlayOptions?.mediaSource;
      if (mediaSource) {
        mediaSource.DefaultAudioStreamIndex = value;
        const audioMpvId = selectedAudioMpvId(mediaSource, value);
        if (audioMpvId !== undefined) {
          sendPlayerCommand({ command: 'set-audio-stream', audioMpvId });
        }
      }
    }

    canSetAudioStreamIndex() {
      return true;
    }

    setSubtitleStreamIndex(index) {
      const value = numberish(index);
      const mediaSource = this._currentPlayOptions?.mediaSource;
      if (mediaSource) {
        mediaSource.DefaultSubtitleStreamIndex = value;
        const selection = selectedSubtitleTrack(mediaSource, value);
        sendPlayerCommand({
          command: 'set-subtitle-stream',
          subtitleMpvId: selection.subtitleMpvId,
          subtitleUrl: selection.subtitleUrl
        });
      }
    }

    setSecondarySubtitleStreamIndex() {}
    resetSubtitleOffset() { this._currentSubtitleOffset = 0; this._showSubtitleOffset = false; }
    enableShowingSubtitleOffset() { this._showSubtitleOffset = true; }
    disableShowingSubtitleOffset() { this._showSubtitleOffset = false; }
    isShowingSubtitleOffsetEnabled() { return this._showSubtitleOffset === true; }
    setSubtitleOffset(value) { this._currentSubtitleOffset = Number(value) || 0; }
    getSubtitleOffset() { return this._currentSubtitleOffset || 0; }
    canHandleOffsetOnCurrentSubtitle() { return true; }
    supportSubtitleOffset() { return true; }

    isFullscreen() { return Boolean(document.fullscreenElement); }
    toggleFullscreen() {}
    setPictureInPictureEnabled() {}
    isPictureInPictureEnabled() { return false; }
    togglePictureInPicture() {}
    setAirPlayEnabled() {}
    isAirPlayEnabled() { return false; }
    toggleAirPlay() {}
    setBrightness() {}
    getBrightness() { return 100; }
    getStats() { return Promise.resolve({ categories: [] }); }
    getSupportedAspectRatios() {
      const translate = (value) => this.globalize?.translate?.(value) || value;
      return [
        { id: 'auto', name: translate('Auto') },
        { id: 'cover', name: translate('AspectRatioCover') },
        { id: 'fill', name: translate('AspectRatioFill') }
      ];
    }
    getAspectRatio() { return this._currentAspectRatio || 'auto'; }
    setAspectRatio(value) { this._currentAspectRatio = value || 'auto'; }
  }

  function existingNativePlugins(nativeShell) {
    try {
      const value = nativeShell?.getPlugins?.();
      return Array.isArray(value) ? value : [];
    } catch (_) {
      return [];
    }
  }

  function installNativeShellPlugin() {
    const nativeShell = window.NativeShell && typeof window.NativeShell === 'object'
      ? window.NativeShell
      : {};
    const plugins = Array.from(new Set([...existingNativePlugins(nativeShell), PLAYER_PLUGIN_NAME]));
    nativeShell.getPlugins = () => plugins.slice();

    if (typeof nativeShell.openUrl !== 'function') {
      nativeShell.openUrl = (url, target) => window.open(url, target || '_blank', 'noopener');
    }
    if (typeof nativeShell.downloadFile !== 'function') {
      nativeShell.downloadFile = (info) => info?.url && nativeShell.openUrl(info.url);
    }
    if (typeof nativeShell.openClientSettings !== 'function') {
      nativeShell.openClientSettings = () => {};
    }

    const exitApplication = () => {
      window.location.href = 'jellyfin-mpv://app-exit';
    };

    const appHost = nativeShell.AppHost && typeof nativeShell.AppHost === 'object'
      ? nativeShell.AppHost
      : {};
    const defaults = {
      init: () => Promise.resolve({
        deviceName: 'jellyfin-mpv',
        appName: 'jellyfin-mpv',
        appVersion: '0.1.0'
      }),
      getDefaultLayout: () => 'desktop',
      supports: (command) => [
        'fileinput',
        'filedownload',
        'displaylanguage',
        'htmlaudioautoplay',
        'htmlvideoautoplay',
        'externallinks',
        'multiserver',
        'fullscreenchange',
        'remotevideo',
        'displaymode',
        'exitmenu'
      ].includes(String(command || '').toLowerCase()),
      getDeviceProfile: () => cloneMpvDeviceProfile(),
      getSyncProfile: () => cloneMpvDeviceProfile(),
      appName: () => 'jellyfin-mpv',
      appVersion: () => '0.1.0',
      deviceName: () => 'jellyfin-mpv',
      exit: exitApplication
    };
    for (const [key, value] of Object.entries(defaults)) {
      if (typeof appHost[key] !== 'function') appHost[key] = value;
    }

    const previousSupports = appHost.supports.bind(appHost);
    appHost.supports = (command) => {
      const feature = String(command || '').toLowerCase();
      return feature === 'exitmenu' || previousSupports(command);
    };

    nativeShell.AppHost = appHost;
    window.NativeShell = nativeShell;
    window[PLAYER_PLUGIN_NAME] = () => JellyfinMpvPlayer;
    window.initCompleted = window.initCompleted || Promise.resolve();
    window.apiPromise = window.apiPromise || Promise.resolve({});
  }

  installNativeShellPlugin();

  if (typeof nativeFetch === 'function') {
    window.fetch = function(input, init) {
      const requestUrl = absoluteUrl(typeof input === 'string' || input instanceof URL ? input : input?.url);
      let requestBody = init?.body;
      let fetchInit = init;
      const patchedBody = patchPlaybackInfoBody(requestUrl, requestBody);
      if (patchedBody !== requestBody) {
        requestBody = patchedBody;
        fetchInit = Object.assign({}, init || {}, { body: patchedBody });
      }
      if (shouldSuppressPlaystateReport(requestUrl, requestBody)) {
        return Promise.resolve(syntheticNoContentResponse(requestUrl));
      }
      if (isDirectStreamUrl(requestUrl)) rememberForStream(requestUrl);
      return nativeFetch.call(this, input, fetchInit).then((response) => {
        if (isPlaybackInfoUrl(requestUrl)) {
          response.clone().json().then((json) => rememberPlaybackInfo(requestUrl, json, requestBody)).catch(() => {});
        }
        return response;
      });
    };
  }

  const nativeOpen = XMLHttpRequest.prototype.open;
  const nativeSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function(method, url) {
    this.__jellyfinMpvMethod = String(method || 'GET').toUpperCase();
    this.__jellyfinMpvUrl = absoluteUrl(url);
    if (isDirectStreamUrl(this.__jellyfinMpvUrl)) rememberForStream(this.__jellyfinMpvUrl);
    return nativeOpen.apply(this, arguments);
  };
  XMLHttpRequest.prototype.send = function(body) {
    let requestBody = patchPlaybackInfoBody(this.__jellyfinMpvUrl, body);
    if (shouldSuppressPlaystateReport(this.__jellyfinMpvUrl, requestBody)) {
      completeSyntheticXhr(this, this.__jellyfinMpvUrl);
      return;
    }
    if (isPlaybackInfoUrl(this.__jellyfinMpvUrl)) {
      this.addEventListener('loadend', () => {
        try {
          rememberPlaybackInfo(this.__jellyfinMpvUrl, JSON.parse(this.responseText), requestBody);
        } catch (_) {}
      });
    }
    return nativeSend.call(this, requestBody);
  };

  const mediaSrc = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'src');
  if (mediaSrc && mediaSrc.set && mediaSrc.get) {
    Object.defineProperty(HTMLMediaElement.prototype, 'src', {
      configurable: true,
      enumerable: mediaSrc.enumerable,
      get() {
        return externalizedContextForNode(this)?.mediaUrl || mediaSrc.get.call(this);
      },
      set(value) {
        const context = rememberForStream(value);
        if (context) {
          markMediaExternalized(this, context);
          sendExternalPlayback(context);
          return;
        }
        markMediaExternalized(this, null);
        return mediaSrc.set.call(this, value);
      }
    });
  }

  patchMediaProperty('currentSrc', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      return externalizedContextForNode(this)?.mediaUrl || descriptor.get.call(this);
    }
  }));

  const nativeCanPlayType = HTMLMediaElement.prototype.canPlayType;
  if (typeof nativeCanPlayType === 'function') {
    HTMLMediaElement.prototype.canPlayType = function(type) {
      const result = nativeCanPlayType.apply(this, arguments);
      if (result) return result;
      return canPlayExternally(type) ? 'probably' : result;
    };
  }

  function patchMediaProperty(name, descriptorFactory) {
    const descriptor = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, name);
    if (!descriptor || descriptor.configurable === false) return;
    Object.defineProperty(HTMLMediaElement.prototype, name, descriptorFactory(descriptor));
  }

  patchMediaProperty('paused', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      const state = syntheticStateFor(this);
      return state ? state.paused : descriptor.get.call(this);
    }
  }));

  patchMediaProperty('ended', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      const state = syntheticStateFor(this);
      return state ? state.ended : descriptor.get.call(this);
    }
  }));

  patchMediaProperty('currentTime', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      const state = syntheticStateFor(this);
      return state ? syntheticCurrentTime(state) : descriptor.get.call(this);
    },
    set(value) {
      const state = syntheticStateFor(this);
      if (state) {
        setSyntheticCurrentTime(state, value);
        dispatchMediaEvent(this, 'seeking');
        dispatchMediaEvent(this, 'seeked');
        dispatchMediaEvent(this, 'timeupdate');
        return;
      }
      return descriptor.set.call(this, value);
    }
  }));

  patchMediaProperty('duration', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      const state = syntheticStateFor(this);
      return state && state.duration > 0 ? state.duration : descriptor.get.call(this);
    }
  }));

  patchMediaProperty('error', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      return syntheticStateFor(this) ? null : descriptor.get.call(this);
    }
  }));

  patchMediaProperty('readyState', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      return syntheticStateFor(this) ? 4 : descriptor.get.call(this);
    }
  }));

  patchMediaProperty('networkState', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      return syntheticStateFor(this) ? 1 : descriptor.get.call(this);
    }
  }));

  patchMediaProperty('seekable', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      const state = syntheticStateFor(this);
      return state ? fakeTimeRanges(state.duration) : descriptor.get.call(this);
    }
  }));

  patchMediaProperty('buffered', (descriptor) => ({
    configurable: true,
    enumerable: descriptor.enumerable,
    get() {
      const state = syntheticStateFor(this);
      return state ? fakeTimeRanges(syntheticCurrentTime(state)) : descriptor.get.call(this);
    }
  }));

  const nativeSetAttribute = Element.prototype.setAttribute;
  const nativeGetAttribute = Element.prototype.getAttribute;
  const nativeHasAttribute = Element.prototype.hasAttribute;
  Element.prototype.getAttribute = function(name) {
    if (String(name).toLowerCase() === 'src') {
      const context = externalizedContextForNode(this);
      if (context?.mediaUrl) return context.mediaUrl;
    }
    return nativeGetAttribute.apply(this, arguments);
  };
  Element.prototype.hasAttribute = function(name) {
    if (String(name).toLowerCase() === 'src' && externalizedContextForNode(this)?.mediaUrl) {
      return true;
    }
    return nativeHasAttribute.apply(this, arguments);
  };
  Element.prototype.setAttribute = function(name, value) {
    if (this instanceof HTMLMediaElement && String(name).toLowerCase() === 'src') {
      const context = rememberForStream(value);
      if (context) {
        markMediaExternalized(this, context);
        sendExternalPlayback(context);
        return;
      }
      markMediaExternalized(this, null);
    } else if (typeof HTMLSourceElement !== 'undefined' && this instanceof HTMLSourceElement && String(name).toLowerCase() === 'src') {
      const context = rememberForStream(value);
      if (context) {
        markSourceExternalized(this, context);
        sendExternalPlayback(context);
        return;
      }
      markSourceExternalized(this, null);
    }
    return nativeSetAttribute.apply(this, arguments);
  };

  if (typeof HTMLSourceElement !== 'undefined') {
    const sourceSrc = Object.getOwnPropertyDescriptor(HTMLSourceElement.prototype, 'src');
    if (sourceSrc && sourceSrc.set && sourceSrc.get) {
      Object.defineProperty(HTMLSourceElement.prototype, 'src', {
        configurable: true,
        enumerable: sourceSrc.enumerable,
        get() {
          return externalizedContextForNode(this)?.mediaUrl || sourceSrc.get.call(this);
        },
        set(value) {
          const context = rememberForStream(value);
          if (context) {
            markSourceExternalized(this, context);
            sendExternalPlayback(context);
            return;
          }
          markSourceExternalized(this, null);
          return sourceSrc.set.call(this, value);
        }
      });
    }
  }

  const nativeMediaLoad = HTMLMediaElement.prototype.load;
  if (typeof nativeMediaLoad === 'function') {
    HTMLMediaElement.prototype.load = function() {
      if (mediaWasExternalized(this)) {
        const state = ensureSyntheticMediaState(this, externalizedMedia.get(this));
        if (state) {
          dispatchMediaEvent(this, 'loadedmetadata');
          dispatchMediaEvent(this, 'durationchange');
          dispatchMediaEvent(this, 'loadeddata');
          dispatchMediaEvent(this, 'canplay');
          dispatchMediaEvent(this, 'canplaythrough');
        }
        return;
      }
      return nativeMediaLoad.apply(this, arguments);
    };
  }

  const nativeMediaPause = HTMLMediaElement.prototype.pause;
  if (typeof nativeMediaPause === 'function') {
    HTMLMediaElement.prototype.pause = function() {
      if (mediaWasExternalized(this)) {
        pauseSyntheticPlayback(this);
        return;
      }
      return nativeMediaPause.apply(this, arguments);
    };
  }

  const nativeMediaPlay = HTMLMediaElement.prototype.play;
  if (typeof nativeMediaPlay === 'function') {
    HTMLMediaElement.prototype.play = function() {
      if (mediaWasExternalized(this)) {
        return playSyntheticMedia(this);
      }
      try {
        const result = nativeMediaPlay.apply(this, arguments);
        if (mediaWasExternalized(this)) {
          if (result && typeof result.catch === 'function') result.catch(() => {});
          return playSyntheticMedia(this);
        }
        return result;
      } catch (error) {
        if (mediaWasExternalized(this)) {
          return playSyntheticMedia(this);
        }
        throw error;
      }
    };
  }

  for (const eventName of ['error', 'abort', 'stalled']) {
    document.addEventListener(eventName, (event) => {
      const media = mediaElementForNode(event.target);
      if (media && mediaWasExternalized(media)) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    }, true);
  }

  console.debug('[jellyfin-mpv] Jellyfin Web bridge installed');
})();
