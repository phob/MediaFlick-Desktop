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
  host.style.cssText = 'position:fixed;left:0;top:0;width:100vw;height:100vh;z-index:2147483647';
  document.documentElement.appendChild(host);

  const root = host.attachShadow({{ mode: 'closed' }});
  root.innerHTML = `
    <style>
      * {{ margin: 0; padding: 0; box-sizing: border-box; }}
      :host {{ all: initial; color-scheme: dark; }}
      .bg {{ position: fixed; inset: 0; background: rgba(0, 0, 0, .5); }}
      .box {{
        position: fixed;
        left: 50%;
        top: 50%;
        transform: translate(-50%, -50%);
        background: #2b2b2b;
        border: 1px solid #555;
        border-radius: 8px;
        padding: 20px 24px 18px;
        min-width: 420px;
        max-width: 80vw;
        font: 13px/1.4 sans-serif;
        color: #e0e0e0;
        box-shadow: 0 4px 24px rgba(0, 0, 0, .6);
      }}
      .head {{ display: flex; justify-content: center; margin-bottom: 16px; }}
      .logo {{ display: block; width: min(240px, 56vw); height: auto; }}
      .x {{
        position: absolute;
        top: 8px;
        right: 10px;
        cursor: pointer;
        padding: 2px 8px;
        border-radius: 4px;
        font-size: 18px;
        line-height: 1;
        color: #aaa;
        user-select: none;
      }}
      .x:hover, .x:focus-visible {{ background: #3d3d3d; color: #e0e0e0; outline: none; }}
      .row {{ display: flex; margin-top: 8px; }}
      .row .k {{ flex: 0 0 140px; color: #888; }}
      .row .v {{ flex: 1 1 auto; word-break: break-all; }}
      @media (max-width: 480px) {{
        .box {{ min-width: 0; width: calc(100vw - 32px); }}
        .row {{ display: block; }}
        .row .k {{ margin-bottom: 2px; }}
      }}
    </style>
    <div class="bg"></div>
    <div class="box" role="dialog" aria-modal="true" aria-labelledby="about-title">
      <div class="head">
        <svg class="logo" viewBox="0 0 620 180" role="img" aria-labelledby="about-title">
          <title id="about-title">Jellyfin MPV</title>
          <defs>
            <linearGradient id="jellyfinMpvAboutGradient" x1="268" y1="220" x2="780" y2="804" gradientUnits="userSpaceOnUse">
              <stop stop-color="#AA5CC3"/>
              <stop offset="1" stop-color="#00A4DC"/>
            </linearGradient>
            <linearGradient id="jellyfinMpvAboutSurface" x1="184" y1="112" x2="840" y2="912" gradientUnits="userSpaceOnUse">
              <stop stop-color="#2A2A38"/>
              <stop offset="1" stop-color="#1D1D27"/>
            </linearGradient>
          </defs>
          <g transform="translate(10 15) scale(.146)">
            <rect x="96" y="96" width="832" height="832" rx="210" fill="url(#jellyfinMpvAboutSurface)"/>
            <rect x="96" y="96" width="832" height="832" rx="210" stroke="#626276" stroke-opacity="0.65" stroke-width="24"/>
            <path fill="url(#jellyfinMpvAboutGradient)" d="M364 292C330 272 288 296 288 336V688C288 728 330 752 364 732L664 556C698 536 698 488 664 468L364 292Z"/>
            <path fill="url(#jellyfinMpvAboutGradient)" fill-opacity="0.88" d="M680 256H796C836 256 868 288 868 328V444H772V352H680V256Z"/>
            <path fill="#20202B" fill-opacity="0.78" d="M384 404V620L568 512L384 404Z"/>
          </g>
          <text x="188" y="86" fill="#e8e8e8" font-family="Inter, Segoe UI, sans-serif" font-size="56" font-weight="700" letter-spacing="-2">Jellyfin</text>
          <text x="191" y="132" fill="#aaa" font-family="Inter, Segoe UI, sans-serif" font-size="34" font-weight="600" letter-spacing="4">MPV</text>
        </svg>
        <div class="x" role="button" tabindex="0" aria-label="Close">×</div>
      </div>
      <div class="row"><div class="k">App version</div><div class="v">${{info.version || 'unknown'}}</div></div>
      <div class="row"><div class="k">Git version</div><div class="v">${{info.gitVersion || 'unknown'}}</div></div>
      <div class="row"><div class="k">Created by</div><div class="v">${{info.createdBy || 'unknown'}}</div></div>
    </div>`;

  const bg = root.querySelector('.bg');
  const box = root.querySelector('.box');
  const xButton = root.querySelector('.x');
  function close() {{
    document.removeEventListener('keydown', onKeyDown, true);
    host.remove();
  }}
  function onKeyDown(event) {{
    if (event.key === 'Escape') {{
      event.preventDefault();
      close();
      return;
    }}
    if ((event.key === 'Enter' || event.key === ' ') && event.target === xButton) {{
      event.preventDefault();
      close();
    }}
  }}
  xButton.addEventListener('click', close);
  bg.addEventListener('mousedown', (event) => {{ event.preventDefault(); close(); }});
  box.addEventListener('mousedown', (event) => event.stopPropagation());
  document.addEventListener('keydown', onKeyDown, true);
  host.addEventListener('jellyfin-mpv-about-focus', () => xButton.focus());
  xButton.focus();
}})();"##
    )
}
