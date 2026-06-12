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
  const CONTEXT_TTL_MS = 15 * 60 * 1000;
  const EXTERNALIZED_TTL_MS = 12 * 60 * 60 * 1000;
  const MAX_BITRATE = 1000000000;
  const PLACEHOLDER_MEDIA_URL = 'data:audio/wav;base64,UklGRsQAAABXQVZFZm10IBAAAAABAAEAQB8AAIA+AAACABAAZGF0YaAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';

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
        updatedAt: Date.now(),
        timer: 0
      };
      syntheticMediaState.set(element, state);
    } else {
      state.context = mergeContext(state.context, context);
      const duration = syntheticDuration(state.context);
      if (duration > 0) state.duration = duration;
    }
    return state;
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
        clearInterval(current.timer);
        current.timer = 0;
        dispatchMediaEvent(element, 'ended');
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
    state.context.externalizedAt = Date.now();
    dispatchMediaEvent(element, 'loadedmetadata');
    dispatchMediaEvent(element, 'durationchange');
    dispatchMediaEvent(element, 'canplay');
    dispatchMediaEvent(element, 'play');
    dispatchMediaEvent(element, 'playing');
    dispatchMediaEvent(element, 'timeupdate');
    startSyntheticMediaTimer(element, state);
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
      const context = mergeContext(base, {
        mediaSourceId: source.Id || source.MediaSourceId || '',
        runtimeTicks: numberish(source.RunTimeTicks),
        startTimeTicks: numberish(base.startTimeTicks ?? source.StartTimeTicks),
        audioStreamIndex: numberish(base.audioStreamIndex ?? source.DefaultAudioStreamIndex),
        subtitleStreamIndex: numberish(base.subtitleStreamIndex ?? source.DefaultSubtitleStreamIndex),
        title: source.Name || source.Path || base.title
      });
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
      const iframe = document.createElement('iframe');
      iframe.style.display = 'none';
      iframe.src = 'jellyfin-mpv://play-context?payload=' + encodeURIComponent(JSON.stringify(clean));
      (document.documentElement || document.body || document).appendChild(iframe);
      setTimeout(() => iframe.remove(), 1000);
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
      const iframe = document.createElement('iframe');
      iframe.style.display = 'none';
      iframe.src = 'jellyfin-mpv://play?payload=' + encodeURIComponent(JSON.stringify(clean));
      (document.documentElement || document.body || document).appendChild(iframe);
      setTimeout(() => iframe.remove(), 1000);
    } catch (error) {
      console.debug('[jellyfin-mpv] bridge play send failed', error);
    }
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

  const nativeFetch = window.fetch;
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
      get: mediaSrc.get,
      set(value) {
        const context = rememberForStream(value);
        if (context) {
          markMediaExternalized(this, context);
          sendExternalPlayback(context);
          return mediaSrc.set.call(this, PLACEHOLDER_MEDIA_URL);
        }
        markMediaExternalized(this, null);
        return mediaSrc.set.call(this, value);
      }
    });
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
  Element.prototype.setAttribute = function(name, value) {
    if (this instanceof HTMLMediaElement && String(name).toLowerCase() === 'src') {
      const context = rememberForStream(value);
      if (context) {
        markMediaExternalized(this, context);
        sendExternalPlayback(context);
        return nativeSetAttribute.call(this, name, PLACEHOLDER_MEDIA_URL);
      }
      markMediaExternalized(this, null);
    } else if (typeof HTMLSourceElement !== 'undefined' && this instanceof HTMLSourceElement && String(name).toLowerCase() === 'src') {
      const context = rememberForStream(value);
      if (context) {
        markSourceExternalized(this, context);
        sendExternalPlayback(context);
        return nativeSetAttribute.call(this, name, PLACEHOLDER_MEDIA_URL);
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
        get: sourceSrc.get,
        set(value) {
          const context = rememberForStream(value);
          if (context) {
            markSourceExternalized(this, context);
            sendExternalPlayback(context);
            return sourceSrc.set.call(this, PLACEHOLDER_MEDIA_URL);
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
          dispatchMediaEvent(this, 'canplay');
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
