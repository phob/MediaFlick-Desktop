(() => {
  const settings = __MEDIAFLICK_CLIENT_SETTINGS_JSON__;
  const BRIDGE_TOKEN = '{{bridge_token}}';
  const existing = document.getElementById('__mediaFlickDesktopClientSettings');
  if (existing) {
    existing.dispatchEvent(new CustomEvent('mediaflick-desktop-settings-focus'));
    return;
  }

  const host = document.createElement('div');
  host.id = '__mediaFlickDesktopClientSettings';
  host.style.cssText = 'position:fixed;inset:0;z-index:2147483647';
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
        --disabled: oklch(54% .01 260);
        --cyan: oklch(68% .145 227);
        --cyan-pressed: oklch(61% .145 227);
        --cyan-soft: oklch(68% .145 227 / .18);
        --violet: oklch(62% .13 316);
        --danger: oklch(66% .18 31);
        --success: oklch(72% .1 158);
        --shadow: oklch(12% .006 260 / .62);
        font: 14px/1.35 "Noto Sans", "Segoe UI", system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
      }
      * { box-sizing: border-box; }
      .bg {
        position: fixed;
        inset: 0;
        background: oklch(12% .006 260 / .72);
      }
      .dialog {
        position: fixed;
        left: 50%;
        top: 50%;
        transform: translate(-50%, -50%);
        width: min(820px, calc(100vw - 32px));
        max-height: calc(100vh - 32px);
        display: flex;
        flex-direction: column;
        overflow: hidden;
        border: 1px solid var(--border-strong);
        border-radius: 12px;
        background: var(--chrome);
        color: var(--text);
        box-shadow: 0 24px 72px var(--shadow);
      }
      .head {
        display: grid;
        grid-template-columns: auto minmax(0, 1fr) auto;
        align-items: start;
        gap: 14px;
        padding: 22px 24px 18px;
        border-bottom: 1px solid var(--border);
        background: oklch(22% .008 260);
      }
      .mark {
        width: 34px;
        height: 34px;
        flex: 0 0 auto;
      }
      .eyebrow {
        margin: 1px 0 4px;
        color: var(--muted);
        font-size: 12px;
        font-weight: 650;
        letter-spacing: .04em;
        text-transform: uppercase;
      }
      h2 {
        margin: 0;
        color: var(--text);
        font-size: 22px;
        font-weight: 600;
        line-height: 1.2;
        letter-spacing: -.01em;
      }
      .close {
        width: 34px;
        height: 34px;
        border: 1px solid transparent;
        border-radius: 4px;
        color: var(--muted);
        background: transparent;
        font: 22px/1 "Segoe UI", system-ui, sans-serif;
        cursor: pointer;
      }
      .close:hover {
        color: var(--text);
        background: var(--raised);
        border-color: var(--border);
      }
      .close:focus-visible,
      button:focus-visible,
      input:focus-visible,
      select:focus-visible {
        outline: 2px solid var(--cyan);
        outline-offset: 2px;
        box-shadow: 0 0 0 3px var(--cyan-soft);
      }
      form {
        min-height: 0;
        display: flex;
        flex: 1 1 auto;
        flex-direction: column;
      }
      .body {
        overflow: auto;
        overscroll-behavior: contain;
        padding: 18px 24px 20px;
        background: var(--media-black);
      }
      .group {
        margin: 0;
        padding: 17px 18px 18px;
        border: 1px solid var(--border);
        border-radius: 8px;
        background: var(--chrome);
      }
      .group + .group { margin-top: 14px; }
      legend {
        padding: 0 6px;
        margin-left: -6px;
        color: var(--text);
        font-size: 15px;
        font-weight: 650;
      }
      .row {
        display: grid;
        grid-template-columns: 188px minmax(0, 1fr);
        gap: 22px;
        align-items: center;
        padding-top: 15px;
      }
      .row + .row {
        margin-top: 15px;
        border-top: 1px solid oklch(82% .01 260 / .1);
      }
      label {
        display: block;
        margin: 0;
        color: var(--text);
        font-size: 14px;
        font-weight: 650;
        line-height: 1.25;
      }
      .control { min-width: 0; }
      .path-row {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto;
        gap: 10px;
      }
      input,
      select {
        width: 100%;
        height: 40px;
        border: 1px solid var(--border);
        border-radius: 4px;
        padding: 0 12px;
        color: var(--text);
        background: var(--panel);
        font: inherit;
        outline: none;
      }
      input:hover,
      select:hover { border-color: var(--border-strong); background: var(--raised); }
      input::placeholder { color: var(--quiet); }
      input:disabled,
      select:disabled { color: var(--disabled); opacity: .72; }
      datalist { display: none; }
      .getmpv { display: grid; gap: 10px; }
      .getmpv #download-mpv { justify-self: start; min-width: 0; }
      .getmpv .cmd-row {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto;
        gap: 10px;
        align-items: center;
      }
      .getmpv code {
        display: block;
        overflow-x: auto;
        height: 40px;
        line-height: 40px;
        padding: 0 12px;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--panel);
        color: var(--text);
        font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
        font-size: 13px;
        white-space: nowrap;
      }
      .getmpv .help { color: var(--cyan); font-weight: 600; text-decoration: none; }
      .getmpv .help:hover { text-decoration: underline; }
      .getmpv .status:empty { min-height: 0; }
      .actions {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto auto;
        align-items: center;
        gap: 10px;
        padding: 14px 24px 18px;
        border-top: 1px solid var(--border);
        background: oklch(22% .008 260);
      }
      button.action {
        min-width: 96px;
        height: 40px;
        border: 1px solid transparent;
        border-radius: 4px;
        padding: 0 15px;
        color: var(--media-black);
        background: var(--cyan);
        font: inherit;
        font-weight: 700;
        cursor: pointer;
        transition: background-color 160ms ease-out, border-color 160ms ease-out, color 160ms ease-out;
      }
      button.action:hover { background: oklch(72% .145 227); }
      button.action:active { background: var(--cyan-pressed); }
      button.secondary {
        color: var(--text);
        background: var(--panel);
        border-color: var(--border);
      }
      button.secondary:hover { background: var(--raised); border-color: var(--border-strong); }
      button.action:disabled {
        cursor: default;
        color: oklch(82% .01 260);
        background: oklch(42% .01 260);
        border-color: transparent;
      }
      .status {
        min-height: 18px;
        color: var(--muted);
        font-size: 13px;
        font-weight: 600;
      }
      .status:empty { min-height: 0; }
      .status.saved { color: var(--success); }
      .status.error { color: var(--danger); }
      @media (max-width: 680px) {
        .dialog { width: calc(100vw - 20px); max-height: calc(100vh - 20px); }
        .head { grid-template-columns: auto minmax(0, 1fr) auto; padding: 18px 16px 15px; }
        .body { padding: 14px 14px 16px; }
        .group { padding: 15px 14px 16px; }
        .row { grid-template-columns: 1fr; gap: 9px; }
        .path-row { grid-template-columns: 1fr; }
        .actions { grid-template-columns: 1fr; padding: 14px 16px 16px; }
        button.action { width: 100%; }
      }
      @media (prefers-reduced-motion: reduce) {
        button.action { transition: none; }
      }
    </style>
    <div class="bg"></div>
    <section class="dialog" role="dialog" aria-modal="true" aria-labelledby="mediaflick-settings-title">
      <div class="head">
        <svg class="mark" viewBox="0 0 1024 1024" aria-hidden="true" focusable="false">
          <defs>
            <linearGradient id="mediaFlickDesktopSettingsGradient" x1="268" y1="220" x2="780" y2="804" gradientUnits="userSpaceOnUse">
              <stop stop-color="#AA5CC3"/>
              <stop offset="1" stop-color="#00A4DC"/>
            </linearGradient>
            <linearGradient id="mediaFlickDesktopSettingsSurface" x1="184" y1="112" x2="840" y2="912" gradientUnits="userSpaceOnUse">
              <stop stop-color="#2A2A38"/>
              <stop offset="1" stop-color="#1D1D27"/>
            </linearGradient>
          </defs>
          <rect x="96" y="96" width="832" height="832" rx="210" fill="url(#mediaFlickDesktopSettingsSurface)"/>
          <rect x="96" y="96" width="832" height="832" rx="210" stroke="#626276" stroke-opacity="0.65" stroke-width="24"/>
          <path fill="url(#mediaFlickDesktopSettingsGradient)" d="M364 292C330 272 288 296 288 336V688C288 728 330 752 364 732L664 556C698 536 698 488 664 468L364 292Z"/>
          <path fill="url(#mediaFlickDesktopSettingsGradient)" fill-opacity="0.88" d="M680 256H796C836 256 868 288 868 328V444H772V352H680V256Z"/>
          <path fill="#20202B" fill-opacity="0.78" d="M384 404V620L568 512L384 404Z"/>
        </svg>
        <div>
          <p class="eyebrow">MediaFlick Desktop</p>
          <h2 id="mediaflick-settings-title">Client settings</h2>
        </div>
        <button class="close" type="button" aria-label="Close">×</button>
      </div>
      <form id="settings-form" aria-busy="false">
        <div class="body">
          <fieldset class="group">
            <legend>mpv handoff</legend>
            <div class="row">
              <label for="mpv-path">mpv executable</label>
              <div class="control path-row">
                <input id="mpv-path" name="mpv-path" type="text" spellcheck="false" autocomplete="off">
                <button id="browse" class="action secondary" type="button">Browse</button>
              </div>
            </div>
            <div class="row">
              <label for="download-mpv">Get mpv</label>
              <div class="control getmpv">
                <button id="download-mpv" class="action secondary" type="button" hidden>Download mpv</button>
                <div class="cmd-row" id="mpv-cmd-line" hidden>
                  <code id="mpv-cmd"></code>
                  <button id="copy-mpv" class="action secondary" type="button">Copy</button>
                </div>
                <span class="status" id="mpv-setup-status" aria-live="polite"></span>
                <a id="mpv-help-link" class="help" href="#">mpv.io/installation</a>
              </div>
            </div>
            <div class="row">
              <label for="default-fullscreen">Default fullscreen</label>
              <div class="control">
                <select id="default-fullscreen" name="default-fullscreen">
                  <option value="fullscreen">Start mpv fullscreen</option>
                  <option value="windowed">Start mpv windowed</option>
                </select>
              </div>
            </div>
            <div class="row">
              <label for="mark-watched-next">Mark watched key</label>
              <div class="control">
                <input id="mark-watched-next" name="mark-watched-next" type="text" spellcheck="false" autocomplete="off" placeholder="w">
              </div>
            </div>
          </fieldset>

          <fieldset class="group">
            <legend>Segment skipping</legend>
            <div class="row">
              <label for="skip-intro">Intros</label>
              <div class="control">
                <select id="skip-intro" name="skip-intro">
                  <option value="disabled">Never skip</option>
                  <option value="prompt">Ask / seek to skip</option>
                  <option value="always">Always skip</option>
                </select>
              </div>
            </div>
            <div class="row">
              <label for="skip-credits">Credits</label>
              <div class="control">
                <select id="skip-credits" name="skip-credits">
                  <option value="disabled">Never skip</option>
                  <option value="prompt">Ask / seek to skip</option>
                  <option value="always">Always skip</option>
                </select>
              </div>
            </div>
            <div class="row">
              <label for="skip-recap">Recaps</label>
              <div class="control">
                <select id="skip-recap" name="skip-recap">
                  <option value="disabled">Never skip</option>
                  <option value="prompt">Ask / seek to skip</option>
                  <option value="always">Always skip</option>
                </select>
              </div>
            </div>
            <div class="row">
              <label for="skip-commercial">Commercials</label>
              <div class="control">
                <select id="skip-commercial" name="skip-commercial">
                  <option value="disabled">Never skip</option>
                  <option value="prompt">Ask / seek to skip</option>
                  <option value="always">Always skip</option>
                </select>
              </div>
            </div>
          </fieldset>

          <fieldset class="group">
            <legend>Desktop shell</legend>
            <div class="row">
              <label for="close-behavior">Close button</label>
              <div class="control">
                <select id="close-behavior" name="close-behavior">
                  <option value="exit_app">Exit application</option>
                  <option value="minimize_window">Minimize window</option>
                </select>
              </div>
            </div>
            <div class="row">
              <label for="scrollbars">Scrollbars</label>
              <div class="control">
                <select id="scrollbars" name="scrollbars">
                  <option value="hidden">Hide scrollbars</option>
                  <option value="visible">Show scrollbars</option>
                </select>
              </div>
            </div>
          </fieldset>

          <fieldset class="group">
            <legend>Diagnostics</legend>
            <div class="row">
              <label for="log-level">Log level</label>
              <div class="control">
                <input id="log-level" name="log-level" type="text" spellcheck="false" autocomplete="off" list="log-level-options">
                <datalist id="log-level-options">
                  <option value="error"></option>
                  <option value="warn"></option>
                  <option value="info"></option>
                  <option value="debug"></option>
                  <option value="trace"></option>
                </datalist>
              </div>
            </div>
          </fieldset>
        </div>
        <div class="actions">
          <span class="status" id="status" aria-live="polite"></span>
          <button class="action secondary" id="cancel" type="button">Cancel</button>
          <button class="action" id="save" type="submit">Save settings</button>
        </div>
      </form>
    </section>`;

  const form = root.getElementById('settings-form');
  const mpvPath = root.getElementById('mpv-path');
  const logLevel = root.getElementById('log-level');
  const defaultFullscreen = root.getElementById('default-fullscreen');
  const closeBehavior = root.getElementById('close-behavior');
  const scrollbars = root.getElementById('scrollbars');
  const skipIntro = root.getElementById('skip-intro');
  const skipCredits = root.getElementById('skip-credits');
  const skipRecap = root.getElementById('skip-recap');
  const skipCommercial = root.getElementById('skip-commercial');
  const markWatchedNext = root.getElementById('mark-watched-next');
  const status = root.getElementById('status');
  const save = root.getElementById('save');
  const browse = root.getElementById('browse');
  const downloadMpv = root.getElementById('download-mpv');
  const mpvCmdLine = root.getElementById('mpv-cmd-line');
  const mpvCmd = root.getElementById('mpv-cmd');
  const copyMpv = root.getElementById('copy-mpv');
  const mpvSetupStatus = root.getElementById('mpv-setup-status');
  const mpvHelpLink = root.getElementById('mpv-help-link');
  const cancel = root.getElementById('cancel');
  const closeButton = root.querySelector('.close');
  const bg = root.querySelector('.bg');

  mpvPath.value = settings.mpvPath || '';
  logLevel.value = settings.logLevel || 'debug';
  defaultFullscreen.value = settings.defaultFullscreen || 'fullscreen';
  closeBehavior.value = settings.closeBehavior || 'exit_app';
  scrollbars.value = settings.showScrollbars ? 'visible' : 'hidden';
  skipIntro.value = settings.skipIntro || 'prompt';
  skipCredits.value = settings.skipCredits || 'prompt';
  skipRecap.value = settings.skipRecap || 'prompt';
  skipCommercial.value = settings.skipCommercial || 'prompt';
  markWatchedNext.value = settings.markWatchedNext || '';

  function focusableControls() {
    return Array.from(root.querySelectorAll('button, input, select')).filter(element => !element.disabled && element.offsetParent !== null);
  }
  function close() {
    document.removeEventListener('keydown', onKeyDown, true);
    delete window.__mediaFlickDesktopSetBusy;
    delete window.__mediaFlickDesktopSetMpvPath;
    delete window.__mediaFlickDesktopClientSettingsSaved;
    delete window.__mediaFlickDesktopClientSettingsSaveFailed;
    delete window.__mediaFlickDesktopMpvSetup;
    host.remove();
  }
  function setStatus(message, kind) {
    status.textContent = message || '';
    status.className = kind ? 'status ' + kind : 'status';
  }
  function setBusy(isBusy, message) {
    save.disabled = isBusy;
    browse.disabled = isBusy;
    form.setAttribute('aria-busy', isBusy ? 'true' : 'false');
    setStatus(isBusy ? (message || 'Saving settings...') : '', '');
  }
  function onKeyDown(event) {
    if (event.key === 'Escape') {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== 'Tab') {
      return;
    }
    const controls = focusableControls();
    if (!controls.length) {
      return;
    }
    const first = controls[0];
    const last = controls[controls.length - 1];
    const active = root.activeElement || document.activeElement;
    if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }
  window.__mediaFlickDesktopSetBusy = isBusy => setBusy(isBusy);
  window.__mediaFlickDesktopSetMpvPath = path => {
    mpvPath.value = path || '';
    setBusy(false);
    mpvPath.focus();
  };
  window.__mediaFlickDesktopClientSettingsSaved = () => {
    setBusy(false);
    setStatus('Saved.', 'saved');
  };
  window.__mediaFlickDesktopClientSettingsSaveFailed = message => {
    setBusy(false);
    setStatus(message || 'Could not save settings.', 'error');
  };

  const mpvCommands = { macos: 'brew install mpv', linux: 'sudo apt install mpv' };
  let mpvSetupBusy = false;

  function mpvBytes(value) {
    const number = Number(value || 0);
    if (!Number.isFinite(number) || number <= 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    let scaled = number;
    let unit = 0;
    while (scaled >= 1024 && unit < units.length - 1) { scaled /= 1024; unit += 1; }
    return `${scaled >= 10 || unit === 0 ? scaled.toFixed(0) : scaled.toFixed(1)} ${units[unit]}`;
  }

  function setMpvSetupStatus(message, kind) {
    mpvSetupStatus.textContent = message || '';
    mpvSetupStatus.className = kind ? 'status ' + kind : 'status';
  }

  if (settings.mpvCanDownload) {
    downloadMpv.hidden = false;
  } else if (mpvCommands[settings.mpvPlatform]) {
    mpvCmd.textContent = mpvCommands[settings.mpvPlatform];
    mpvCmdLine.hidden = false;
  }

  downloadMpv.addEventListener('click', () => {
    if (mpvSetupBusy) return;
    mpvSetupBusy = true;
    downloadMpv.disabled = true;
    downloadMpv.textContent = 'Starting';
    setMpvSetupStatus('Contacting mpv release server…', '');
    window.location.href = 'mediaflick-desktop://mpv-download?token=' + BRIDGE_TOKEN;
  });

  copyMpv.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(mpvCmd.textContent);
    } catch (error) {
      const range = document.createRange();
      range.selectNodeContents(mpvCmd);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      try { document.execCommand('copy'); } catch (ignored) {}
      selection.removeAllRanges();
    }
    copyMpv.textContent = 'Copied';
    setTimeout(() => { copyMpv.textContent = 'Copy'; }, 1500);
  });

  mpvHelpLink.addEventListener('click', event => {
    event.preventDefault();
    window.location.href = 'mediaflick-desktop://mpv-help?token=' + BRIDGE_TOKEN;
  });

  window.__mediaFlickDesktopMpvSetup = event => {
    if (!event) return;
    const state = event.state;
    const payload = event.payload || {};
    if (state === 'downloading') {
      mpvSetupBusy = true;
      downloadMpv.disabled = true;
      downloadMpv.textContent = 'Downloading';
      const downloaded = Number(payload.downloaded || 0);
      const total = Number(payload.total || 0);
      setMpvSetupStatus(total > 0
        ? `Downloading mpv — ${mpvBytes(downloaded)} of ${mpvBytes(total)}`
        : `Downloading mpv — ${mpvBytes(downloaded)}`, '');
    } else if (state === 'extracting') {
      downloadMpv.textContent = 'Installing';
      setMpvSetupStatus('Installing mpv…', '');
    } else if (state === 'done') {
      mpvSetupBusy = false;
      downloadMpv.disabled = false;
      downloadMpv.textContent = 'Download mpv';
      setMpvSetupStatus('mpv installed.', 'saved');
      if (payload.path) mpvPath.value = payload.path;
    } else if (state === 'error') {
      mpvSetupBusy = false;
      downloadMpv.disabled = false;
      downloadMpv.textContent = 'Download mpv';
      setMpvSetupStatus(payload.message || 'Could not download mpv.', 'error');
    }
  };

  browse.addEventListener('click', () => {
    setBusy(true, 'Opening file picker...');
    window.location.href = 'mediaflick-desktop://select-mpv?token=' + BRIDGE_TOKEN + '&target=settings';
  });
  form.addEventListener('submit', event => {
    event.preventDefault();
    if (!mpvPath.value.trim()) {
      setStatus('Choose an mpv executable before saving.', 'error');
      mpvPath.focus();
      return;
    }
    setBusy(true, 'Saving settings...');
    const query = new URLSearchParams({
      token: BRIDGE_TOKEN,
      mpv: mpvPath.value.trim(),
      logLevel: logLevel.value,
      defaultFullscreen: defaultFullscreen.value,
      closeBehavior: closeBehavior.value,
      scrollbars: scrollbars.value,
      skipIntro: skipIntro.value,
      skipCredits: skipCredits.value,
      skipRecap: skipRecap.value,
      skipCommercial: skipCommercial.value,
      markWatchedNext: markWatchedNext.value.trim()
    });
    window.location.href = `mediaflick-desktop://client-settings-save?${query.toString()}`;
  });
  cancel.addEventListener('click', close);
  closeButton.addEventListener('click', close);
  bg.addEventListener('mousedown', event => { event.preventDefault(); close(); });
  host.addEventListener('mediaflick-desktop-settings-focus', () => mpvPath.focus());
  document.addEventListener('keydown', onKeyDown, true);
  mpvPath.focus();
})();