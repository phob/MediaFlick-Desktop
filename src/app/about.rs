use serde_json::json;

pub const APP_NAME: &str = "jellyfin-mpv";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_VERSION: &str = env!("JELLYFIN_MPV_GIT_VERSION");
pub const CREATED_BY: &str = env!("JELLYFIN_MPV_CREATED_BY");

pub fn info_json() -> serde_json::Value {
    json!({
        "appName": APP_NAME,
        "version": APP_VERSION,
        "gitVersion": GIT_VERSION,
        "createdBy": CREATED_BY,
    })
}

pub fn dialog_script() -> String {
    let data = info_json();
    format!(
        r##"(() => {{
  const info = {data};
  const existing = document.getElementById('__jellyfinMpvAbout');
  if (existing) {{
    existing.dispatchEvent(new CustomEvent('jellyfin-mpv-about-focus'));
    return;
  }}

  const host = document.createElement('div');
  host.id = '__jellyfinMpvAbout';
  host.style.position = 'fixed';
  host.style.inset = '0';
  host.style.zIndex = '2147483647';
  document.documentElement.appendChild(host);

  const root = host.attachShadow({{ mode: 'closed' }});
  root.innerHTML = `
    <style>
      :host {{ all: initial; color-scheme: dark; }}
      .backdrop {{
        position: fixed;
        inset: 0;
        display: grid;
        place-items: center;
        padding: 24px;
        background: oklch(8% 0.012 273 / 0.72);
        font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      }}
      .dialog {{
        width: min(92vw, 420px);
        color: oklch(92% 0.012 273);
        border: 1px solid oklch(100% 0 0 / 0.12);
        border-radius: 24px;
        background: linear-gradient(180deg, oklch(25% 0.03 273), oklch(18% 0.022 273));
        box-shadow: 0 32px 90px oklch(0% 0 0 / 0.48);
        overflow: hidden;
        animation: enter 180ms cubic-bezier(.16, 1, .3, 1) both;
      }}
      .header {{
        display: flex;
        gap: 14px;
        align-items: center;
        padding: 22px 22px 10px;
      }}
      .mark {{
        width: 46px;
        height: 46px;
        flex: 0 0 auto;
        filter: drop-shadow(0 14px 26px oklch(55% 0.19 285 / 0.42));
      }}
      h1 {{
        margin: 0;
        font-size: 1.45rem;
        line-height: 1;
        letter-spacing: -0.035em;
      }}
      .subtitle {{ margin-top: 5px; color: oklch(72% 0.025 273); font-size: .9rem; }}
      dl {{ margin: 10px 22px 20px; }}
      .row {{
        display: grid;
        grid-template-columns: 104px 1fr;
        gap: 14px;
        padding: 11px 0;
        border-top: 1px solid oklch(100% 0 0 / 0.07);
      }}
      dt {{ color: oklch(70% 0.025 273); font-weight: 680; }}
      dd {{ margin: 0; min-width: 0; overflow-wrap: anywhere; font-variant-numeric: tabular-nums; }}
      .actions {{ display: flex; justify-content: flex-end; padding: 0 22px 22px; }}
      button {{
        height: 42px;
        border: 0;
        border-radius: 13px;
        padding: 0 18px;
        color: oklch(98% 0.006 240);
        background: linear-gradient(135deg, oklch(70% 0.18 225), oklch(67% 0.2 295));
        font: inherit;
        font-weight: 760;
        cursor: pointer;
      }}
      button:focus-visible {{ outline: 3px solid oklch(70% 0.18 225 / 0.42); outline-offset: 3px; }}
      @keyframes enter {{ from {{ opacity: 0; transform: translateY(10px) scale(.98); }} to {{ opacity: 1; transform: none; }} }}
    </style>
    <div class="backdrop" part="backdrop">
      <section class="dialog" role="dialog" aria-modal="true" aria-labelledby="about-title">
        <div class="header">
          <svg class="mark" viewBox="0 0 128 128" role="img" aria-label="Jellyfin logo mark">
            <defs><linearGradient id="g" x1="10" x2="118" y1="118" y2="10" gradientUnits="userSpaceOnUse"><stop stop-color="#00a4dc"/><stop offset="1" stop-color="#aa5cff"/></linearGradient></defs>
            <path fill="url(#g)" d="M64 6 122 64 64 122 6 64 64 6Zm0 28L34 64l30 30 30-30-30-30Zm0 18 12 12-12 12-12-12 12-12Z"/>
          </svg>
          <div>
            <h1 id="about-title">${{info.appName}}</h1>
            <div class="subtitle">External mpv companion for Jellyfin</div>
          </div>
        </div>
        <dl>
          <div class="row"><dt>Version</dt><dd>${{info.version || 'unknown'}}</dd></div>
          <div class="row"><dt>Git version</dt><dd>${{info.gitVersion || 'unknown'}}</dd></div>
          <div class="row"><dt>Created by</dt><dd>${{info.createdBy || 'unknown'}}</dd></div>
        </dl>
        <div class="actions"><button type="button" autofocus>Close</button></div>
      </section>
    </div>`;

  const button = root.querySelector('button');
  const backdrop = root.querySelector('.backdrop');
  const dialog = root.querySelector('.dialog');
  function close() {{
    document.removeEventListener('keydown', onKeyDown, true);
    host.remove();
  }}
  function onKeyDown(event) {{
    if (event.key === 'Escape') {{
      event.preventDefault();
      close();
    }}
  }}
  button.addEventListener('click', close);
  backdrop.addEventListener('click', (event) => {{ if (!dialog.contains(event.target)) close(); }});
  document.addEventListener('keydown', onKeyDown, true);
  host.addEventListener('jellyfin-mpv-about-focus', () => button.focus());
  button.focus();
}})();"##
    )
}
