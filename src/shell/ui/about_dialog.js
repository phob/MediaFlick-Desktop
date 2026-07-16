(() => {
  const info = __MEDIAFLICK_ABOUT_INFO_JSON__;
  const existing = document.getElementById('__mediaFlickDesktopAbout');
  if (existing) {
    existing.dispatchEvent(new CustomEvent('mediaflick-desktop-about-focus'));
    return;
  }

  const host = document.createElement('div');
  host.id = '__mediaFlickDesktopAbout';
  host.style.cssText = 'position:fixed;left:0;top:0;width:100vw;height:100vh;z-index:2147483647';
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
        --violet: oklch(62% .13 316);
        --shadow: oklch(12% .006 260 / .62);
        font: 14px/1.35 "Noto Sans", "Segoe UI", system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
      }
      * { box-sizing: border-box; }
      .bg {
        position: fixed;
        inset: 0;
        background: color-mix(in oklch, var(--media-black) 72%, transparent);
      }
      .box {
        position: fixed;
        left: 50%;
        top: 50%;
        transform: translate(-50%, -50%);
        width: min(480px, calc(100vw - 32px));
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
        position: relative;
        display: grid;
        grid-template-columns: auto minmax(0, 1fr) auto;
        gap: 14px;
        align-items: start;
        padding: 22px 22px 18px;
        border-bottom: 1px solid var(--border);
        background: oklch(22% .008 260);
      }
      .mark {
        width: 42px;
        height: 42px;
        flex: 0 0 auto;
        filter: drop-shadow(0 10px 24px var(--shadow));
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
        transition: background-color 160ms ease-out, border-color 160ms ease-out, color 160ms ease-out;
      }
      .close:hover {
        color: var(--text);
        background: var(--raised);
        border-color: var(--border);
      }
      .close:active {
        background: var(--panel);
      }
      .close:focus-visible {
        outline: 2px solid var(--cyan);
        outline-offset: 2px;
        box-shadow: 0 0 0 3px oklch(68% .145 227 / .18);
      }
      .body {
        min-height: 0;
        overflow: auto;
        padding: 18px 22px 20px;
        background: var(--media-black);
      }
      dl {
        display: grid;
        grid-template-columns: 126px minmax(0, 1fr);
        gap: 0 18px;
        margin: 0;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--chrome);
        overflow: hidden;
      }
      dt,
      dd {
        margin: 0;
        padding: 11px 13px;
        border-top: 1px solid var(--border);
      }
      dt:nth-of-type(1),
      dd:nth-of-type(1) { border-top: 0; }
      dt {
        color: var(--quiet);
        font-size: 12px;
        font-weight: 650;
        letter-spacing: .02em;
        text-transform: uppercase;
      }
      dd {
        min-width: 0;
        color: var(--text);
        word-break: break-all;
      }
      @media (max-width: 480px) {
        .box { width: calc(100vw - 20px); max-height: calc(100vh - 20px); }
        .head { padding: 18px 16px 15px; }
        .close { width: 40px; height: 40px; }
        .body { padding: 14px 16px 16px; }
        dl { grid-template-columns: 1fr; }
        dt { padding-bottom: 2px; }
        dd { padding-top: 0; border-top: 0; }
        dt:not(:first-of-type) { border-top: 1px solid var(--border); }
      }
      @media (prefers-reduced-motion: reduce) {
        .close { transition: none; }
      }
    </style>
    <div class="bg"></div>
    <section class="box" role="dialog" aria-modal="true" aria-labelledby="about-title">
      <div class="head">
        <svg class="mark" viewBox="0 0 1024 1024" aria-hidden="true" focusable="false">
          <defs>
            <linearGradient id="mediaFlickDesktopAboutGradient" x1="268" y1="220" x2="780" y2="804" gradientUnits="userSpaceOnUse">
              <stop stop-color="#AA5CC3"/>
              <stop offset="1" stop-color="#00A4DC"/>
            </linearGradient>
            <linearGradient id="mediaFlickDesktopAboutSurface" x1="184" y1="112" x2="840" y2="912" gradientUnits="userSpaceOnUse">
              <stop stop-color="#2A2A38"/>
              <stop offset="1" stop-color="#1D1D27"/>
            </linearGradient>
          </defs>
          <rect x="96" y="96" width="832" height="832" rx="210" fill="url(#mediaFlickDesktopAboutSurface)"/>
          <rect x="96" y="96" width="832" height="832" rx="210" stroke="#626276" stroke-opacity="0.65" stroke-width="24"/>
          <path fill="url(#mediaFlickDesktopAboutGradient)" d="M364 292C330 272 288 296 288 336V688C288 728 330 752 364 732L664 556C698 536 698 488 664 468L364 292Z"/>
          <path fill="url(#mediaFlickDesktopAboutGradient)" fill-opacity="0.88" d="M680 256H796C836 256 868 288 868 328V444H772V352H680V256Z"/>
          <path fill="#20202B" fill-opacity="0.78" d="M384 404V620L568 512L384 404Z"/>
        </svg>
        <div>
          <p class="eyebrow">About</p>
          <h2 id="about-title">${info.appName || 'MediaFlick Desktop'}</h2>
        </div>
        <button class="close" type="button" aria-label="Close">×</button>
      </div>
      <div class="body">
        <dl>
          <dt>App version</dt>
          <dd>${info.version || 'unknown'}</dd>
          <dt>Git version</dt>
          <dd>${info.gitVersion || 'unknown'}</dd>
          <dt>Created by</dt>
          <dd>${info.createdBy || 'unknown'}</dd>
        </dl>
      </div>
    </section>`;

  const bg = root.querySelector('.bg');
  const box = root.querySelector('.box');
  const closeButton = root.querySelector('.close');
  function close() {
    document.removeEventListener('keydown', onKeyDown, true);
    host.remove();
  }
  function focusableControls() {
    return Array.from(root.querySelectorAll('button')).filter(element => !element.disabled);
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
  closeButton.addEventListener('click', close);
  bg.addEventListener('mousedown', (event) => { event.preventDefault(); close(); });
  box.addEventListener('mousedown', (event) => event.stopPropagation());
  document.addEventListener('keydown', onKeyDown, true);
  host.addEventListener('mediaflick-desktop-about-focus', () => closeButton.focus());
  closeButton.focus();
})();