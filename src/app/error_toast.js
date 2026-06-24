(() => {
  const payload = {{error_payload}};
  const hostId = '__mediaFlickDesktopErrorToast';

  const COPY_ICON = '<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
  const DONE_ICON = '<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>';

  const existing = document.getElementById(hostId);
  if (existing && window.__mediaFlickDesktopShowError) {
    window.__mediaFlickDesktopShowError(payload);
    return;
  }

  let dismissed = false;
  let copyResetTimer = 0;
  let current = {};

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
        --chrome: oklch(20% .01 260);
        --raised: oklch(34% .012 260);
        --border: oklch(78% .01 260 / .16);
        --text: oklch(91% .01 260);
        --muted: oklch(73% .012 260);
        --danger: oklch(66% .18 31);
        --danger-soft: oklch(66% .18 31 / .18);
        --shadow: oklch(12% .006 260 / .58);
        font: 13px/1.35 "Noto Sans", "Segoe UI", system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
      }
      * { box-sizing: border-box; }
      .toast {
        position: relative;
        display: grid;
        grid-template-columns: 28px minmax(0, 1fr) 24px;
        align-items: start;
        gap: 10px;
        width: min(380px, calc(100vw - 36px));
        min-height: 56px;
        overflow: hidden;
        padding: 12px;
        border: 1px solid oklch(66% .18 31 / .54);
        border-radius: 14px;
        background: var(--chrome);
        color: var(--text);
        box-shadow: 0 14px 34px var(--shadow), 0 1px 0 oklch(96% .006 260 / .07) inset;
      }
      .mark {
        width: 28px;
        height: 28px;
        display: grid;
        place-items: center;
        border: 1px solid var(--border);
        border-radius: 999px;
        background: var(--danger-soft);
        color: var(--danger);
        font-size: 16px;
        font-weight: 800;
        line-height: 1;
      }
      .content { min-width: 0; align-self: center; }
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
        margin: 2px 0 0;
        color: var(--muted);
        overflow-wrap: anywhere;
      }
      .actions {
        display: grid;
        justify-items: center;
        gap: 6px;
      }
      .icon-btn {
        appearance: none;
        display: grid;
        place-items: center;
        width: 24px;
        height: 24px;
        border: 0;
        border-radius: 999px;
        background: transparent;
        color: var(--muted);
        cursor: pointer;
        transition: background-color 160ms ease-out, color 160ms ease-out;
      }
      .icon-btn:hover { background: var(--raised); color: var(--text); }
      .icon-btn:focus-visible {
        outline: 2px solid var(--danger);
        outline-offset: 2px;
      }
      .close { font: 18px/1 "Segoe UI", system-ui, sans-serif; }
      .copy-btn.done { color: var(--danger); }
    </style>
    <div class="toast" role="alert" aria-live="assertive">
      <div class="mark">!</div>
      <div class="content">
        <p class="title"></p>
        <p class="body"></p>
      </div>
      <div class="actions">
        <button class="icon-btn close" type="button" aria-label="Dismiss error notification">×</button>
        <button class="icon-btn copy-btn" type="button" aria-label="Copy error" title="Copy error">${COPY_ICON}</button>
      </div>
    </div>`;

  const title = root.querySelector('.title');
  const body = root.querySelector('.body');
  const copyBtn = root.querySelector('.copy-btn');
  const close = root.querySelector('.close');

  function dismiss() {
    dismissed = true;
    if (copyResetTimer) clearTimeout(copyResetTimer);
    host.remove();
  }

  function copyText() {
    return [current.title, current.body].filter(Boolean).join('\n');
  }

  function fallbackCopy(text) {
    const area = document.createElement('textarea');
    area.value = text;
    area.style.cssText = 'position:fixed;opacity:0;pointer-events:none';
    root.appendChild(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand('copy');
    } catch (_) {
      ok = false;
    }
    area.remove();
    return ok;
  }

  function resetCopy() {
    copyBtn.innerHTML = COPY_ICON;
    copyBtn.classList.remove('done');
    copyBtn.title = 'Copy error';
    copyBtn.setAttribute('aria-label', 'Copy error');
  }

  async function copy() {
    const text = copyText();
    let ok = false;
    try {
      if (navigator.clipboard && navigator.clipboard.writeText) {
        await navigator.clipboard.writeText(text);
        ok = true;
      } else {
        ok = fallbackCopy(text);
      }
    } catch (_) {
      ok = fallbackCopy(text);
    }
    if (ok) {
      copyBtn.innerHTML = DONE_ICON;
      copyBtn.classList.add('done');
      copyBtn.title = 'Copied';
      copyBtn.setAttribute('aria-label', 'Copied');
    } else {
      copyBtn.title = 'Copy failed — select and press Ctrl+C';
    }
    if (copyResetTimer) clearTimeout(copyResetTimer);
    copyResetTimer = setTimeout(resetCopy, 2000);
  }

  function render(next) {
    if (dismissed) return;
    current = {
      title: String(next.title || 'Something went wrong'),
      body: String(next.body || ''),
    };
    title.textContent = current.title;
    body.textContent = current.body;
    body.hidden = !current.body;
    resetCopy();
  }

  copyBtn.addEventListener('click', copy);
  close.addEventListener('click', dismiss);
  window.__mediaFlickDesktopShowError = (next) => render(next || {});
  render(payload);
})();
