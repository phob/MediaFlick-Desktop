(() => {
  const update = {{update_payload}};
  const hostId = '__mediaFlickDesktopUpdateToast';
  const existing = document.getElementById(hostId);
  if (existing && window.__mediaFlickDesktopShowUpdateToast) {
    window.__mediaFlickDesktopShowUpdateToast(update);
    return;
  }

  let status = 'available';
  let progressPayload = {};
  let dismissed = false;

  const host = document.createElement('div');
  host.id = hostId;
  host.style.cssText = 'position:fixed;right:18px;bottom:18px;z-index:2147483647';
  document.documentElement.appendChild(host);

  const root = host.attachShadow({ mode: 'closed' });
  root.innerHTML = `
    <style>
      :host {
        all: initial;
        color-scheme: dark;
        --media-black: oklch(18% .006 260);
        --chrome: oklch(24% .008 260);
        --panel: oklch(29% .01 260);
        --raised: oklch(34% .012 260);
        --border: oklch(78% .01 260 / .16);
        --border-strong: oklch(82% .012 260 / .26);
        --text: oklch(91% .01 260);
        --muted: oklch(73% .012 260);
        --quiet: oklch(59% .012 260);
        --cyan: oklch(68% .145 227);
        --cyan-hover: oklch(72% .145 227);
        --cyan-pressed: oklch(61% .145 227);
        --cyan-soft: oklch(68% .145 227 / .18);
        --danger: oklch(66% .18 31);
        --shadow: oklch(12% .006 260 / .62);
        font: 14px/1.35 "Noto Sans", "Segoe UI", system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
      }
      * { box-sizing: border-box; }
      .toast {
        width: min(380px, calc(100vw - 36px));
        overflow: hidden;
        border: 1px solid var(--border-strong);
        border-radius: 12px;
        background: var(--chrome);
        color: var(--text);
        box-shadow: 0 24px 72px var(--shadow);
      }
      .top { display: flex; gap: 12px; padding: 14px 14px 12px; }
      .mark {
        flex: 0 0 32px;
        width: 32px;
        height: 32px;
        display: grid;
        place-items: center;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--cyan-soft);
        color: var(--cyan);
        font-size: 18px;
        font-weight: 700;
        line-height: 1;
      }
      .copy { min-width: 0; flex: 1 1 auto; }
      .title {
        margin: 0 0 3px;
        color: var(--text);
        font-size: 14px;
        font-weight: 700;
        line-height: 1.25;
        letter-spacing: -.01em;
      }
      .body { margin: 0; color: var(--muted); }
      .asset {
        margin: 5px 0 0;
        color: var(--quiet);
        font-size: 12px;
        line-height: 1.35;
        word-break: break-all;
      }
      .asset:empty { display: none; }
      .close {
        appearance: none;
        width: 34px;
        height: 34px;
        border: 1px solid transparent;
        border-radius: 4px;
        background: transparent;
        color: var(--muted);
        cursor: pointer;
        font: 22px/1 "Segoe UI", system-ui, sans-serif;
        transition: background-color 160ms ease-out, border-color 160ms ease-out, color 160ms ease-out;
      }
      .close:hover { background: var(--raised); border-color: var(--border); color: var(--text); }
      .close:active { background: var(--panel); }
      .actions { display: flex; align-items: center; gap: 10px; padding: 0 14px 14px 58px; }
      .primary {
        appearance: none;
        min-height: 36px;
        border: 1px solid transparent;
        border-radius: 4px;
        padding: 0 13px;
        background: var(--cyan);
        color: var(--media-black);
        cursor: pointer;
        font: 700 13px/1 "Noto Sans", "Segoe UI", system-ui, sans-serif;
        transition: background-color 160ms ease-out, border-color 160ms ease-out, color 160ms ease-out;
      }
      .primary:hover { background: var(--cyan-hover); }
      .primary:active { background: var(--cyan-pressed); }
      .close:focus-visible,
      .primary:focus-visible {
        outline: 2px solid var(--cyan);
        outline-offset: 2px;
        box-shadow: 0 0 0 3px var(--cyan-soft);
      }
      .primary[disabled],
      .primary[disabled]:hover {
        cursor: default;
        color: var(--muted);
        background: var(--panel);
        border-color: transparent;
      }
      .meter { height: 6px; overflow: hidden; border-top: 1px solid var(--border); background: var(--media-black); }
      .bar {
        width: 100%;
        height: 100%;
        transform: scaleX(0);
        transform-origin: left center;
        background: var(--cyan);
        transition: transform 160ms ease-out;
      }
      .meta { min-height: 17px; color: var(--quiet); font-size: 12px; }
      .error { color: var(--danger); }
      .toast.clickable { cursor: pointer; }
      .toast.clickable:hover .primary { background: var(--cyan-hover); }
      @media (max-width: 480px) {
        .toast { width: calc(100vw - 20px); }
        .top { padding: 14px 12px 12px; }
        .actions { flex-wrap: wrap; padding: 0 12px 12px 56px; }
        .primary { min-height: 40px; }
        .close { width: 40px; height: 40px; }
      }
      @media (prefers-reduced-motion: reduce) {
        .bar,
        .close,
        .primary { transition: none; }
      }
    </style>
    <div class="toast" role="status" aria-live="polite">
      <div class="top">
        <div class="mark">↑</div>
        <div class="copy">
          <p class="title"></p>
          <p class="body"></p>
          <p class="asset"></p>
        </div>
        <button class="close" type="button" aria-label="Dismiss update notification">×</button>
      </div>
      <div class="actions">
        <button class="primary" type="button"></button>
        <span class="meta"></span>
      </div>
      <div class="meter" hidden><div class="bar"></div></div>
    </div>`;

  const toast = root.querySelector('.toast');
  const mark = root.querySelector('.mark');
  const title = root.querySelector('.title');
  const body = root.querySelector('.body');
  const asset = root.querySelector('.asset');
  const primary = root.querySelector('.primary');
  const close = root.querySelector('.close');
  const meter = root.querySelector('.meter');
  const bar = root.querySelector('.bar');
  const meta = root.querySelector('.meta');

  function bytes(value) {
    const number = Number(value || 0);
    if (!Number.isFinite(number) || number <= 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    let scaled = number;
    let unit = 0;
    while (scaled >= 1024 && unit < units.length - 1) {
      scaled /= 1024;
      unit += 1;
    }
    return `${scaled >= 10 || unit === 0 ? scaled.toFixed(0) : scaled.toFixed(1)} ${units[unit]}`;
  }

  function canInstallAutomatically() {
    return Boolean(update.automaticInstall && update.asset?.browserDownloadUrl);
  }

  function sendBridgeAction(action) {
    const frame = document.createElement('iframe');
    frame.style.display = 'none';
    frame.src = `mediaflick-desktop://${action}?version=${encodeURIComponent(update.version || '')}`;
    document.documentElement.appendChild(frame);
    setTimeout(() => frame.remove(), 30000);
  }

  function requestDownload() {
    if (status !== 'available' && status !== 'error') return;
    status = 'downloading';
    progressPayload = { downloaded: 0, total: update.asset?.size || 0 };
    render();
    sendBridgeAction('update-download');
  }

  function openReleasePage() {
    if (status !== 'available' && status !== 'error') return;
    sendBridgeAction('update-release');
  }

  function activateUpdate() {
    if (canInstallAutomatically()) requestDownload();
    else openReleasePage();
  }

  function dismiss() {
    dismissed = true;
    host.remove();
  }

  function render() {
    if (dismissed) return;
    toast.classList.toggle('clickable', status === 'available');
    close.hidden = status === 'downloading' || status === 'installing';
    meter.hidden = status === 'available' || status === 'error';
    meta.className = 'meta';
    bar.style.transform = 'scaleX(0)';
    mark.textContent = status === 'error' ? '!' : canInstallAutomatically() ? '↑' : '↗';
    primary.disabled = false;

    if (status === 'available') {
      title.textContent = `MediaFlick Desktop ${update.version} is available`;
      if (canInstallAutomatically()) {
        body.textContent = 'Click to download and install it quietly. The app will restart when setup finishes.';
        asset.textContent = update.asset?.name || '';
        primary.textContent = 'Install update';
      } else {
        body.textContent = 'Open the latest GitHub release to download the update for this platform.';
        asset.textContent = update.releasePageUrl || update.htmlUrl || '';
        primary.textContent = 'Open latest release';
      }
      meta.textContent = '';
      return;
    }

    if (status === 'downloading') {
      const downloaded = Number(progressPayload.downloaded || 0);
      const total = Number(progressPayload.total || 0);
      const percent = total > 0 ? Math.max(0, Math.min(100, (downloaded / total) * 100)) : 0;
      title.textContent = `Downloading MediaFlick Desktop ${update.version}`;
      body.textContent = 'Keep the app open while the installer is downloaded.';
      asset.textContent = update.asset?.name || '';
      primary.textContent = 'Downloading';
      primary.disabled = true;
      mark.textContent = '↓';
      bar.style.transform = `scaleX(${percent / 100})`;
      meta.textContent = total > 0 ? `${Math.round(percent)}%, ${bytes(downloaded)} of ${bytes(total)}` : bytes(downloaded);
      return;
    }

    if (status === 'installing') {
      title.textContent = 'Installing update';
      body.textContent = 'The app will close now. Setup will run quietly and launch the new version.';
      asset.textContent = update.asset?.name || '';
      primary.textContent = 'Installing';
      primary.disabled = true;
      mark.textContent = '✓';
      bar.style.transform = 'scaleX(1)';
      meta.textContent = 'Ready';
      return;
    }

    title.textContent = 'Update failed';
    body.textContent = String(progressPayload.message || 'Could not download or start the installer.');
    asset.textContent = update.releasePageUrl || update.htmlUrl || '';
    primary.textContent = 'Try again';
    meter.hidden = true;
    meta.className = 'meta error';
    meta.textContent = 'Error';
  }

  primary.addEventListener('click', (event) => {
    event.preventDefault();
    event.stopPropagation();
    activateUpdate();
  });
  toast.addEventListener('click', () => activateUpdate());
  close.addEventListener('click', (event) => {
    event.preventDefault();
    event.stopPropagation();
    dismiss();
  });
  window.__mediaFlickDesktopShowUpdateToast = (nextUpdate) => {
    if (dismissed) return;
    Object.assign(update, nextUpdate || {});
    status = status === 'error' ? 'available' : status;
    render();
  };
  window.__mediaFlickDesktopUpdateProgress = (event) => {
    if (!event) return;
    status = event.state || status;
    progressPayload = event.payload || {};
    render();
  };
  render();
})();
