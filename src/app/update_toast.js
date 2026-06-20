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
        --chrome: oklch(20% .01 260);
        --panel: oklch(29% .01 260);
        --raised: oklch(34% .012 260);
        --border: oklch(78% .01 260 / .16);
        --border-strong: oklch(68% .145 227 / .46);
        --text: oklch(91% .01 260);
        --muted: oklch(73% .012 260);
        --cyan: oklch(68% .145 227);
        --cyan-hover: oklch(72% .145 227);
        --cyan-soft: oklch(68% .145 227 / .18);
        --danger: oklch(66% .18 31);
        --danger-soft: oklch(66% .18 31 / .18);
        --shadow: oklch(12% .006 260 / .58);
        font: 13px/1.35 "Noto Sans", "Segoe UI", system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
      }
      * { box-sizing: border-box; }
      .toast {
        position: relative;
        display: grid;
        grid-template-columns: 28px minmax(0, 1fr) auto 24px;
        align-items: center;
        gap: 10px;
        width: min(336px, calc(100vw - 36px));
        min-height: 56px;
        overflow: hidden;
        padding: 10px;
        border: 1px solid var(--border-strong);
        border-radius: 999px;
        background: var(--chrome);
        color: var(--text);
        box-shadow: 0 14px 34px var(--shadow), 0 1px 0 oklch(96% .006 260 / .07) inset;
      }
      .toast.error-state { border-color: oklch(66% .18 31 / .54); }
      .mark {
        width: 28px;
        height: 28px;
        display: grid;
        place-items: center;
        border: 1px solid var(--border);
        border-radius: 999px;
        background: var(--cyan-soft);
        color: var(--cyan);
        font-size: 16px;
        font-weight: 800;
        line-height: 1;
      }
      .toast.error-state .mark { background: var(--danger-soft); color: var(--danger); }
      .copy { min-width: 0; }
      .title {
        margin: 0;
        overflow: hidden;
        color: var(--text);
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 13px;
        font-weight: 700;
        line-height: 1.25;
        letter-spacing: -.01em;
      }
      .body {
        margin: 1px 0 0;
        overflow: hidden;
        color: var(--muted);
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .close {
        appearance: none;
        width: 24px;
        height: 24px;
        border: 0;
        border-radius: 999px;
        background: transparent;
        color: var(--muted);
        cursor: pointer;
        font: 18px/1 "Segoe UI", system-ui, sans-serif;
        transition: background-color 160ms ease-out, color 160ms ease-out;
      }
      .close:hover { background: var(--raised); color: var(--text); }
      .primary {
        appearance: none;
        min-height: 32px;
        border: 0;
        border-radius: 999px;
        padding: 0 12px;
        background: var(--cyan);
        color: var(--media-black);
        cursor: pointer;
        font: 800 12px/1 "Noto Sans", "Segoe UI", system-ui, sans-serif;
        white-space: nowrap;
        transition: background-color 160ms ease-out, color 160ms ease-out;
      }
      .primary:hover { background: var(--cyan-hover); }
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
      }
      .meter {
        position: absolute;
        left: 16px;
        right: 16px;
        bottom: 4px;
        height: 3px;
        overflow: hidden;
        border-radius: 999px;
        background: var(--media-black);
        pointer-events: none;
      }
      .bar {
        width: 100%;
        height: 100%;
        transform: scaleX(0);
        transform-origin: left center;
        background: var(--cyan);
        transition: transform 160ms cubic-bezier(.22, 1, .36, 1);
      }
      .toast.clickable { cursor: pointer; }
      .toast.clickable:hover .primary { background: var(--cyan-hover); }
      @media (max-width: 480px) {
        .toast {
          grid-template-columns: 28px minmax(0, 1fr) auto 28px;
          width: calc(100vw - 20px);
          min-height: 60px;
        }
        .primary { min-height: 36px; }
        .close { width: 28px; height: 28px; }
      }
      @media (prefers-reduced-motion: reduce) {
        .bar,
        .close,
        .primary { transition: none; }
      }
    </style>
    <div class="toast" role="status" aria-live="polite">
      <div class="mark">↑</div>
      <div class="copy">
        <p class="title"></p>
        <p class="body"></p>
      </div>
      <button class="primary" type="button"></button>
      <button class="close" type="button" aria-label="Dismiss update notification">×</button>
      <div class="meter" hidden><div class="bar"></div></div>
    </div>`;

  const toast = root.querySelector('.toast');
  const mark = root.querySelector('.mark');
  const title = root.querySelector('.title');
  const body = root.querySelector('.body');
  const primary = root.querySelector('.primary');
  const close = root.querySelector('.close');
  const meter = root.querySelector('.meter');
  const bar = root.querySelector('.bar');

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
    const automaticInstall = canInstallAutomatically();
    toast.classList.toggle('clickable', status === 'available');
    toast.classList.toggle('error-state', status === 'error');
    close.hidden = status === 'downloading' || status === 'installing';
    meter.hidden = status === 'available' || status === 'error';
    bar.style.transform = 'scaleX(0)';
    mark.textContent = status === 'error' ? '!' : automaticInstall ? '↑' : '↗';
    primary.disabled = false;

    if (status === 'available') {
      title.textContent = 'Update available';
      body.textContent = `MediaFlick Desktop ${update.version}`;
      primary.textContent = automaticInstall ? 'Install' : 'Open release';
      return;
    }

    if (status === 'downloading') {
      const downloaded = Number(progressPayload.downloaded || 0);
      const total = Number(progressPayload.total || 0);
      const percent = total > 0 ? Math.max(0, Math.min(100, (downloaded / total) * 100)) : 0;
      title.textContent = 'Downloading update';
      body.textContent = total > 0 ? `${bytes(downloaded)} of ${bytes(total)}` : bytes(downloaded);
      primary.textContent = total > 0 ? `${Math.round(percent)}%` : 'Working';
      primary.disabled = true;
      mark.textContent = '↓';
      bar.style.transform = `scaleX(${percent / 100})`;
      return;
    }

    if (status === 'installing') {
      title.textContent = 'Installing update';
      body.textContent = 'MediaFlick Desktop will restart.';
      primary.textContent = 'Ready';
      primary.disabled = true;
      mark.textContent = '✓';
      bar.style.transform = 'scaleX(1)';
      return;
    }

    title.textContent = 'Update failed';
    body.textContent = String(progressPayload.message || 'Could not download or start the installer.');
    primary.textContent = automaticInstall ? 'Try again' : 'Open release';
    meter.hidden = true;
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
