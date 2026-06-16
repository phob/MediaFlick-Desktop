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
      * { box-sizing: border-box; }
      :host { all: initial; color-scheme: dark; }
      .toast {
        width: min(360px, calc(100vw - 36px));
        border: 1px solid color-mix(in oklch, oklch(72% .13 244) 48%, oklch(25% .02 260));
        border-radius: 14px;
        background: linear-gradient(145deg, oklch(24% .026 266), oklch(18% .018 270));
        box-shadow: 0 18px 44px rgba(5, 8, 18, .48), 0 1px 0 rgba(238, 242, 255, .08) inset;
        color: oklch(92% .01 260);
        font: 13px/1.45 Segoe UI, Inter, system-ui, sans-serif;
        overflow: hidden;
      }
      .top { display: flex; gap: 12px; padding: 15px 15px 13px; }
      .mark {
        flex: 0 0 32px;
        width: 32px;
        height: 32px;
        display: grid;
        place-items: center;
        border-radius: 10px;
        background: oklch(55% .17 260 / .22);
        color: oklch(78% .16 235);
        font-size: 18px;
        font-weight: 700;
      }
      .copy { min-width: 0; flex: 1 1 auto; }
      .title { font-size: 14px; font-weight: 700; letter-spacing: -.01em; margin: 0 0 2px; }
      .body { color: oklch(77% .025 260); margin: 0; }
      .asset { color: oklch(66% .035 260); margin-top: 4px; word-break: break-all; }
      .close {
        appearance: none;
        border: 0;
        background: transparent;
        color: oklch(72% .025 260);
        cursor: pointer;
        font: 18px/1 Segoe UI, system-ui, sans-serif;
        height: 26px;
        width: 26px;
        border-radius: 8px;
      }
      .close:hover, .close:focus-visible { background: oklch(34% .03 260); color: oklch(94% .01 260); outline: none; }
      .actions { display: flex; align-items: center; gap: 10px; padding: 0 15px 15px 59px; }
      .primary {
        appearance: none;
        border: 0;
        border-radius: 10px;
        padding: 8px 12px;
        background: linear-gradient(135deg, oklch(64% .18 246), oklch(58% .18 292));
        color: oklch(98% .005 260);
        cursor: pointer;
        font: 700 12px/1 Segoe UI, Inter, system-ui, sans-serif;
      }
      .primary:hover, .primary:focus-visible { filter: brightness(1.08); outline: none; }
      .primary[disabled] { cursor: default; filter: saturate(.4) brightness(.75); }
      .meter { height: 8px; background: oklch(15% .018 270); overflow: hidden; }
      .bar { height: 100%; width: 0%; background: linear-gradient(90deg, oklch(68% .18 235), oklch(67% .17 292)); transition: width 160ms ease-out; }
      .meta { color: oklch(68% .035 260); font-size: 12px; min-height: 17px; }
      .error { color: oklch(78% .14 28); }
      .toast.clickable { cursor: pointer; }
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

  function requestDownload() {
    if (status !== 'available' && status !== 'error') return;
    status = 'downloading';
    progressPayload = { downloaded: 0, total: update.asset?.size || 0 };
    render();
    const frame = document.createElement('iframe');
    frame.style.display = 'none';
    frame.src = 'mediaflick-desktop://update-download?version=' + encodeURIComponent(update.version || '');
    document.documentElement.appendChild(frame);
    setTimeout(() => frame.remove(), 30000);
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
    bar.style.width = '0%';
    primary.disabled = false;

    if (status === 'available') {
      title.textContent = `MediaFlick Desktop ${update.version} is available`;
      body.textContent = 'Click to download and install it quietly. The app will restart when setup finishes.';
      asset.textContent = update.asset?.name || '';
      primary.textContent = 'Install update';
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
      bar.style.width = `${percent}%`;
      meta.textContent = total > 0 ? `${Math.round(percent)}%, ${bytes(downloaded)} of ${bytes(total)}` : bytes(downloaded);
      return;
    }

    if (status === 'installing') {
      title.textContent = 'Installing update';
      body.textContent = 'The app will close now. Setup will run quietly and launch the new version.';
      asset.textContent = update.asset?.name || '';
      primary.textContent = 'Installing';
      primary.disabled = true;
      bar.style.width = '100%';
      meta.textContent = 'Ready';
      return;
    }

    title.textContent = 'Update failed';
    body.textContent = String(progressPayload.message || 'Could not download or start the installer.');
    asset.textContent = update.htmlUrl || '';
    primary.textContent = 'Try again';
    meter.hidden = true;
    meta.className = 'meta error';
    meta.textContent = 'Error';
  }

  primary.addEventListener('click', (event) => {
    event.preventDefault();
    event.stopPropagation();
    requestDownload();
  });
  toast.addEventListener('click', () => requestDownload());
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
