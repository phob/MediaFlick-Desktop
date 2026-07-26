// MediaFlick Desktop — own UI.
//
// Hand-written ES modules: the whole bundle is embedded in the Rust binary, so
// there is no Node toolchain in the build. Data comes from the local SQLite
// cache through `mediaflick-desktop://app/api/*`; playback goes straight to the
// native playback coordinator.

const TICKS_PER_MS = 10000;
const POSTER_WIDTH = 400;
const PAGE_SIZE = 60;
const PLAYER_POLL_MS = 1000;

// ---------------------------------------------------------------- utilities

const el = document.getElementById("app");

function h(tag, attrs = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === null || value === undefined || value === false) continue;
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of [].concat(children)) {
    if (child === null || child === undefined || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
  return node;
}

async function api(path, options = {}) {
  const init = { method: options.method || "GET", headers: {} };
  if (options.body !== undefined) {
    init.headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(options.body);
  }
  const response = await fetch(path, init);
  let payload = null;
  try {
    payload = await response.json();
  } catch (_) {
    payload = null;
  }
  if (!response.ok) {
    const error = new Error((payload && payload.error) || `request failed (${response.status})`);
    error.status = response.status;
    error.expired = Boolean(payload && payload.expired);
    throw error;
  }
  return payload;
}

function imageUrl(item, type = "Primary", width = POSTER_WIDTH) {
  const tag = type === "Backdrop" ? item.backdropImageTag : item.primaryImageTag;
  if (!tag) return null;
  return `/api/image/${encodeURIComponent(item.id)}/${type}?tag=${encodeURIComponent(tag)}&maxWidth=${width}`;
}

function ticksToMinutes(ticks) {
  if (!ticks) return null;
  return Math.round(ticks / TICKS_PER_MS / 60000);
}

function formatRuntime(ticks) {
  const minutes = ticksToMinutes(ticks);
  if (!minutes) return null;
  const hours = Math.floor(minutes / 60);
  return hours > 0 ? `${hours}h ${minutes % 60}m` : `${minutes}m`;
}

function formatClock(ms) {
  if (!Number.isFinite(ms) || ms < 0) ms = 0;
  const total = Math.floor(ms / 1000);
  const seconds = String(total % 60).padStart(2, "0");
  const minutes = Math.floor(total / 60) % 60;
  const hours = Math.floor(total / 3600);
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
}

function episodeLabel(item) {
  if (item.parentIndexNumber === null || item.indexNumber === null) return null;
  const season = String(item.parentIndexNumber ?? 0).padStart(2, "0");
  const episode = String(item.indexNumber ?? 0).padStart(2, "0");
  return `S${season}E${episode}`;
}

function subtitleFor(item) {
  if (item.kind === "Episode") {
    return [item.seriesName, episodeLabel(item)].filter(Boolean).join(" · ");
  }
  if (item.kind === "Season") return item.seriesName;
  return item.year ? String(item.year) : null;
}

function progressFraction(item) {
  if (!item.positionTicks || !item.runtimeTicks) return 0;
  return Math.min(1, item.positionTicks / item.runtimeTicks);
}

// -------------------------------------------------------------------- state

const state = {
  status: null,
  route: { name: "home" },
  player: { active: false },
  playerTimer: null,
  toasts: [],
};

function toast(message, kind = "info") {
  const entry = { message, kind, id: Math.random().toString(36).slice(2) };
  state.toasts.push(entry);
  renderToasts();
  setTimeout(() => {
    state.toasts = state.toasts.filter((item) => item.id !== entry.id);
    renderToasts();
  }, 6000);
}

function renderToasts() {
  let host = document.querySelector(".toasts");
  if (!host) {
    host = h("div", { class: "toasts" });
    document.body.append(host);
  }
  clear(host);
  for (const entry of state.toasts) {
    host.append(h("div", { class: `toast ${entry.kind}`, text: entry.message }));
  }
}

function reportError(error) {
  if (error && error.expired) {
    state.status = null;
    toast("Your Jellyfin session expired. Please sign in again.", "error");
    navigate("#/login");
    return;
  }
  toast((error && error.message) || "Something went wrong", "error");
}

// ------------------------------------------------------------------- router

function parseRoute() {
  const raw = (location.hash || "#/").replace(/^#/, "");
  const [pathPart, queryPart] = raw.split("?");
  const segments = pathPart.split("/").filter(Boolean);
  const query = new URLSearchParams(queryPart || "");
  if (segments[0] === "login") return { name: "login" };
  if (segments[0] === "library") {
    const search = query.get("search") || "";
    return {
      name: "library",
      // A search spans the whole library; browsing defaults to movies.
      kind: query.has("kind") ? query.get("kind") : search ? "" : "Movie",
      search,
      genre: query.get("genre") || "",
      sort: query.get("sort") || "name",
      watched: query.get("watched") || "",
    };
  }
  if (segments[0] === "item" && segments[1]) return { name: "item", id: segments[1] };
  return { name: "home" };
}

function navigate(hash) {
  if (location.hash === hash) render();
  else location.hash = hash;
}

window.addEventListener("hashchange", () => {
  state.route = parseRoute();
  render();
});

// -------------------------------------------------------------------- views

function renderShell(content, { showChrome = true } = {}) {
  clear(el);
  if (showChrome) el.append(header());
  const main = h("main", { class: "main" }, content);
  el.append(main);
  if (showChrome && state.player.active) el.append(playerBar());
  return main;
}

function header() {
  const route = state.route;
  const searchInput = h("input", {
    type: "search",
    placeholder: "Search your library…",
    value: route.name === "library" ? route.search : "",
    "aria-label": "Search",
  });
  let debounce = null;
  searchInput.addEventListener("input", () => {
    clearTimeout(debounce);
    const value = searchInput.value.trim();
    // Search spans every kind, so the type filter is dropped while searching.
    debounce = setTimeout(() => {
      const params = new URLSearchParams();
      if (value) params.set("search", value);
      navigate(`#/library?${params.toString()}`);
    }, 140);
  });
  // Each keystroke re-renders the shell, so the caret has to be put back.
  searchInput.addEventListener("focus", () => {
    state.searchFocused = true;
  });
  searchInput.addEventListener("blur", () => {
    state.searchFocused = false;
  });
  if (state.searchFocused) {
    queueMicrotask(() => {
      searchInput.focus();
      const end = searchInput.value.length;
      searchInput.setSelectionRange(end, end);
    });
  }

  const link = (label, hash, active) =>
    h("a", {
      href: hash,
      text: label,
      class: active ? "active" : null,
    });

  return h("header", { class: "header" }, [
    h("div", { class: "brand", text: "MediaFlick" }),
    h("nav", { class: "nav" }, [
      link("Home", "#/", route.name === "home"),
      link("Movies", "#/library?kind=Movie", route.name === "library" && route.kind === "Movie"),
      link("Shows", "#/library?kind=Series", route.name === "library" && route.kind === "Series"),
    ]),
    h("div", { class: "search" }, [searchInput]),
    h("div", {
      class: "user",
      text: state.status && state.status.userName ? state.status.userName : "",
    }),
  ]);
}

function card(item) {
  const source = imageUrl(item);
  const poster = h("div", { class: "poster" }, [
    source
      ? h("img", { src: source, alt: item.name, loading: "lazy" })
      : h("div", { class: "fallback", text: item.name }),
    item.played ? h("div", { class: "badge", text: "✓" }) : null,
  ]);
  const fraction = progressFraction(item);
  if (fraction > 0) {
    poster.append(
      h("div", { class: "progress" }, [h("span", { style: `width:${Math.round(fraction * 100)}%` })]),
    );
  }
  const subtitle = subtitleFor(item);
  return h(
    "button",
    {
      class: "card",
      type: "button",
      onclick: () => navigate(`#/item/${encodeURIComponent(item.id)}`),
    },
    [
      poster,
      h("div", { class: "title", text: item.name, title: item.name }),
      subtitle ? h("div", { class: "subtitle", text: subtitle }) : null,
    ],
  );
}

async function homeView() {
  const main = renderShell(h("div", { class: "empty", text: "Loading…" }));
  let payload;
  try {
    payload = await api("/api/home");
  } catch (error) {
    reportError(error);
    return;
  }
  const rows = payload.rows.filter((row) => row.items.length > 0);
  clear(main);
  if (!state.status.bootstrapped) {
    main.append(
      h("div", {
        class: "sync-note",
        text: "Building the local library cache… rows fill in as the sync progresses.",
      }),
    );
  }
  if (rows.length === 0) {
    main.append(
      h("div", {
        class: "empty",
        text: state.status.library && state.status.library.total > 0
          ? "Nothing to continue yet — open Movies or Shows."
          : "No library items cached yet.",
      }),
    );
    return;
  }
  for (const row of rows) {
    main.append(
      h("section", { class: "row" }, [
        h("h2", { text: row.title }),
        h("div", { class: "rail" }, row.items.map(card)),
      ]),
    );
  }
}

// A windowed grid: only the rows on screen exist in the DOM, and item pages
// are fetched on demand, so a 10k-item library scrolls without stutter.
function libraryView() {
  const route = state.route;
  const main = renderShell([]);

  const cardWidth = 168 + 18;
  const cardHeight = 306;
  const pages = new Map();
  const pending = new Set();
  let total = 0;

  const count = h("div", { class: "count", text: "…" });
  const canvas = h("div", { class: "grid-canvas" });
  const viewport = h("div", { class: "grid-viewport" }, [canvas]);

  const update = (patch) => {
    const params = new URLSearchParams();
    const next = { ...route, ...patch };
    for (const key of ["kind", "search", "genre", "sort", "watched"]) {
      if (next[key]) params.set(key, next[key]);
    }
    navigate(`#/library?${params.toString()}`);
  };

  const kindSelect = h(
    "select",
    { onchange: (event) => update({ kind: event.target.value }) },
    [
      h("option", { value: "Movie", text: "Movies", selected: route.kind === "Movie" }),
      h("option", { value: "Series", text: "Shows", selected: route.kind === "Series" }),
      h("option", { value: "Episode", text: "Episodes", selected: route.kind === "Episode" }),
      h("option", { value: "", text: "Everything", selected: route.kind === "" }),
    ],
  );
  const sortSelect = h(
    "select",
    { onchange: (event) => update({ sort: event.target.value }) },
    [
      h("option", { value: "name", text: "Sort: Name", selected: route.sort === "name" }),
      h("option", { value: "year", text: "Sort: Year", selected: route.sort === "year" }),
      h("option", { value: "added", text: "Sort: Recently added", selected: route.sort === "added" }),
      h("option", { value: "rating", text: "Sort: Rating", selected: route.sort === "rating" }),
    ],
  );
  const watchedSelect = h(
    "select",
    { onchange: (event) => update({ watched: event.target.value }) },
    [
      h("option", { value: "", text: "All", selected: route.watched === "" }),
      h("option", { value: "false", text: "Unwatched", selected: route.watched === "false" }),
      h("option", { value: "true", text: "Watched", selected: route.watched === "true" }),
    ],
  );
  const genreSelect = h("select", { onchange: (event) => update({ genre: event.target.value }) }, [
    h("option", { value: "", text: "All genres" }),
  ]);

  main.append(
    h("div", { class: "filters" }, [kindSelect, sortSelect, watchedSelect, genreSelect, count]),
    viewport,
  );

  api("/api/genres")
    .then((payload) => {
      for (const genre of payload.genres) {
        genreSelect.append(
          h("option", { value: genre, text: genre, selected: route.genre === genre }),
        );
      }
    })
    .catch(() => {});

  const columns = () => Math.max(1, Math.floor(viewport.clientWidth / cardWidth));

  const fetchPage = (page) => {
    if (pages.has(page) || pending.has(page)) return;
    pending.add(page);
    const params = new URLSearchParams({
      offset: String(page * PAGE_SIZE),
      limit: String(PAGE_SIZE),
      sort: route.sort,
    });
    if (route.kind) params.set("kind", route.kind);
    if (route.search) params.set("search", route.search);
    if (route.genre) params.set("genre", route.genre);
    if (route.watched) params.set("watched", route.watched);
    api(`/api/items?${params.toString()}`)
      .then((payload) => {
        pending.delete(page);
        pages.set(page, payload.items);
        total = payload.total;
        count.textContent = `${total} item${total === 1 ? "" : "s"}`;
        paint();
      })
      .catch((error) => {
        pending.delete(page);
        reportError(error);
      });
  };

  const paint = () => {
    const cols = columns();
    const rows = Math.ceil(total / cols);
    canvas.style.height = `${rows * cardHeight}px`;
    const scrollTop = main.scrollTop;
    const firstRow = Math.max(0, Math.floor(scrollTop / cardHeight) - 1);
    const lastRow = Math.min(rows, Math.ceil((scrollTop + main.clientHeight) / cardHeight) + 1);
    const firstIndex = firstRow * cols;
    const lastIndex = Math.min(total, lastRow * cols);

    for (let page = Math.floor(firstIndex / PAGE_SIZE); page <= Math.floor(Math.max(0, lastIndex - 1) / PAGE_SIZE); page += 1) {
      fetchPage(page);
    }

    clear(canvas);
    for (let index = firstIndex; index < lastIndex; index += 1) {
      const item = (pages.get(Math.floor(index / PAGE_SIZE)) || [])[index % PAGE_SIZE];
      if (!item) continue;
      const node = card(item);
      node.style.transform = `translate(${(index % cols) * cardWidth}px, ${Math.floor(index / cols) * cardHeight}px)`;
      canvas.append(node);
    }
  };

  main.addEventListener("scroll", paint, { passive: true });
  const onResize = () => paint();
  window.addEventListener("resize", onResize);
  state.cleanup = () => window.removeEventListener("resize", onResize);

  fetchPage(0);
  paint();
}

async function itemView(itemId) {
  const main = renderShell(h("div", { class: "empty", text: "Loading…" }));
  let item;
  try {
    item = await api(`/api/item/${encodeURIComponent(itemId)}`);
  } catch (error) {
    reportError(error);
    return;
  }

  const poster = imageUrl(item, "Primary", 520);
  const meta = [
    item.year,
    formatRuntime(item.runtimeTicks),
    item.officialRating,
    item.communityRating ? `★ ${item.communityRating.toFixed(1)}` : null,
  ]
    .filter(Boolean)
    .join("  ·  ");

  const actions = h("div", { class: "actions" });
  const playable = item.kind === "Movie" || item.kind === "Episode";
  if (playable) {
    const resumable = item.positionTicks > 0 && !item.played;
    if (resumable) {
      actions.append(
        h("button", {
          class: "primary",
          type: "button",
          text: `Resume at ${formatClock(item.positionTicks / TICKS_PER_MS)}`,
          onclick: () => play(item.id, { resume: true }),
        }),
      );
    }
    actions.append(
      h("button", {
        class: resumable ? null : "primary",
        type: "button",
        text: "Play from start",
        onclick: () => play(item.id, { resume: false }),
      }),
    );
  }
  actions.append(
    h("button", {
      type: "button",
      text: item.played ? "Mark unwatched" : "Mark watched",
      onclick: () => setPlayed(item, !item.played),
    }),
    h("button", {
      type: "button",
      text: item.favorite ? "Remove favorite" : "Add favorite",
      onclick: () => setFavorite(item, !item.favorite),
    }),
    qualityPicker(),
  );

  const details = h("div", {}, [
    h("h1", { text: item.name }),
    item.kind === "Episode" && item.seriesName
      ? h("div", { class: "meta", text: `${item.seriesName} · ${episodeLabel(item) || ""}` })
      : null,
    meta ? h("div", { class: "meta", text: meta }) : null,
    actions,
    item.overview ? h("p", { class: "overview", text: item.overview }) : null,
    item.genres && item.genres.length
      ? h(
          "div",
          { class: "chips" },
          item.genres.map((genre) => h("span", { class: "chip", text: genre })),
        )
      : null,
  ]);

  clear(main);
  main.append(
    h("div", { class: "detail" }, [
      h("div", { class: "art" }, [
        poster ? h("img", { src: poster, alt: item.name }) : h("div", { class: "fallback", text: item.name }),
      ]),
      details,
    ]),
  );

  if (item.kind === "Series" || item.kind === "Season") {
    main.append(await childrenSection(item));
  }
  if (item.people && item.people.length) {
    main.append(
      h("section", { class: "section" }, [
        h("h2", { text: "Cast & crew" }),
        h(
          "div",
          { class: "people" },
          item.people.slice(0, 18).map((person) =>
            h("div", { class: "person" }, [
              h("div", { class: "avatar" }, [
                person.imageTag && person.id
                  ? h("img", {
                      src: `/api/image/${encodeURIComponent(person.id)}/Primary?tag=${encodeURIComponent(person.imageTag)}&maxWidth=220`,
                      alt: person.name,
                      loading: "lazy",
                    })
                  : h("div", { class: "fallback", text: person.name || "?" }),
              ]),
              h("div", { text: person.name || "" }),
              person.role ? h("div", { class: "role", text: person.role }) : null,
            ]),
          ),
        ),
      ]),
    );
  }
}

async function childrenSection(item) {
  const section = h("section", { class: "section" }, [
    h("h2", { text: item.kind === "Series" ? "Seasons" : "Episodes" }),
  ]);
  let children = [];
  try {
    children = (await api(`/api/item/${encodeURIComponent(item.id)}/children`)).items;
  } catch (error) {
    reportError(error);
    return section;
  }
  if (children.length === 0) {
    section.append(h("div", { class: "subtitle", text: "Nothing cached here yet." }));
    return section;
  }
  if (item.kind === "Series") {
    section.append(h("div", { class: "rail" }, children.map(card)));
    return section;
  }
  section.append(
    h(
      "div",
      { class: "episodes" },
      children.map((episode) =>
        h(
          "button",
          {
            class: "episode",
            type: "button",
            onclick: () => navigate(`#/item/${encodeURIComponent(episode.id)}`),
          },
          [
            h("span", { class: "number", text: episodeLabel(episode) || "" }),
            h("span", { class: "name", text: episode.name }),
            episode.played ? h("span", { class: "watched", text: "✓" }) : null,
            h("span", { class: "time", text: formatRuntime(episode.runtimeTicks) || "" }),
          ],
        ),
      ),
    ),
  );
  return section;
}

function qualityPicker() {
  const select = h("select", {
    "aria-label": "Streaming quality",
    onchange: (event) => {
      state.quality = event.target.value;
      toast(`Next playback uses ${event.target.selectedOptions[0].textContent}.`);
    },
  });
  const options = [
    ["", "Quality: from settings"],
    ["original", "Original file"],
    ["auto", "Auto"],
    ["20_mbps", "20 Mbps"],
    ["10_mbps", "10 Mbps"],
    ["5_mbps", "5 Mbps"],
    ["3_mbps", "3 Mbps"],
    ["1_5_mbps", "1.5 Mbps"],
  ];
  for (const [value, label] of options) {
    select.append(h("option", { value, text: label, selected: (state.quality || "") === value }));
  }
  return select;
}

// ----------------------------------------------------------------- playback

async function play(itemId, { resume }) {
  try {
    const body = { itemId, resume };
    if (state.quality) body.quality = state.quality;
    const result = await api("/api/play", { method: "POST", body });
    toast(`Playing (${result.playMethod})`);
    state.lastPlayedItemId = itemId;
    startPlayerPolling();
  } catch (error) {
    reportError(error);
  }
}

async function setPlayed(item, played) {
  try {
    await api(`/api/item/${encodeURIComponent(item.id)}/played`, {
      method: "POST",
      body: { played },
    });
    render();
  } catch (error) {
    reportError(error);
  }
}

async function setFavorite(item, favorite) {
  try {
    await api(`/api/item/${encodeURIComponent(item.id)}/favorite`, {
      method: "POST",
      body: { favorite },
    });
    render();
  } catch (error) {
    reportError(error);
  }
}

function playerBar() {
  const player = state.player;
  const fraction =
    player.durationMs > 0 ? Math.min(1, (player.positionMs || 0) / player.durationMs) : 0;
  const scrubber = h("div", { class: "scrubber" }, [
    h("span", { style: `width:${Math.round(fraction * 100)}%` }),
  ]);
  scrubber.addEventListener("click", (event) => {
    if (!player.durationMs) return;
    const bounds = scrubber.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (event.clientX - bounds.left) / bounds.width));
    command("seek", { positionMs: ratio * player.durationMs });
  });

  return h("div", { class: "player-bar" }, [
    h("div", { class: "now", text: state.nowPlayingName || "Playing" }),
    h("button", {
      type: "button",
      text: player.paused ? "Resume" : "Pause",
      onclick: () => command(player.paused ? "resume" : "pause"),
    }),
    h("button", { type: "button", text: "Stop", onclick: () => command("stop") }),
    scrubber,
    h("div", {
      class: "time",
      text: `${formatClock(player.positionMs || 0)} / ${formatClock(player.durationMs || 0)}`,
    }),
  ]);
}

async function command(name, extra = {}) {
  try {
    await api("/api/player/command", { method: "POST", body: { command: name, ...extra } });
    await pollPlayer();
  } catch (error) {
    reportError(error);
  }
}

async function pollPlayer() {
  let snapshot;
  try {
    snapshot = await api("/api/player/state");
  } catch (_) {
    return;
  }
  const wasActive = state.player.active;
  state.player = snapshot;
  if (snapshot.active !== wasActive) render();
  else if (snapshot.active) refreshPlayerBar();
  if (!snapshot.active) stopPlayerPolling();
}

function refreshPlayerBar() {
  const existing = el.querySelector(".player-bar");
  if (existing) existing.replaceWith(playerBar());
}

function startPlayerPolling() {
  if (state.playerTimer) return;
  state.playerTimer = setInterval(pollPlayer, PLAYER_POLL_MS);
  pollPlayer();
}

function stopPlayerPolling() {
  clearInterval(state.playerTimer);
  state.playerTimer = null;
}

// The Rust shell calls this from `dispatch_playback_event` when the external
// player stops, so the UI reacts without waiting for the next poll.
window.__mediaFlickDesktopPlaybackStopped = (payload) => {
  state.player = { active: false };
  stopPlayerPolling();
  const itemId = payload && payload.itemId;
  const reason = payload && payload.stopReason;
  if (itemId && (reason === "eof" || reason === "watched-next")) {
    api("/api/play/next", { method: "POST", body: { itemId } })
      .then((result) => {
        if (result.started) {
          toast("Playing the next episode");
          startPlayerPolling();
        }
        render();
      })
      .catch(() => render());
    return;
  }
  render();
};

// ------------------------------------------------------------------- log in

function loginView() {
  clear(el);
  const status = state.status || {};
  const server = h("input", {
    type: "url",
    value: status.serverUrl || "",
    placeholder: "http://jellyfin.local:8096",
  });
  const username = h("input", { type: "text", autocomplete: "username" });
  const password = h("input", { type: "password", autocomplete: "current-password" });
  const error = h("div", { class: "error" });
  const serverInfo = h("p", { class: "hint", text: "Sign in to your Jellyfin server." });
  const quickHost = h("div");

  const submit = h("button", { type: "submit", text: "Sign in" });
  const quickButton = h("button", {
    type: "button",
    class: "secondary",
    text: "Use Quick Connect",
    onclick: () => startQuickConnect(server.value, quickHost, error),
  });

  const form = h(
    "form",
    {
      onsubmit: async (event) => {
        event.preventDefault();
        error.textContent = "";
        submit.disabled = true;
        submit.textContent = "Signing in…";
        try {
          state.status = await api("/api/auth/login", {
            method: "POST",
            body: {
              server: server.value,
              username: username.value,
              password: password.value,
            },
          });
          navigate("#/");
        } catch (failure) {
          error.textContent =
            failure.status === 401
              ? "Wrong username or password."
              : failure.message || "Sign in failed.";
        } finally {
          submit.disabled = false;
          submit.textContent = "Sign in";
        }
      },
    },
    [
      h("div", { class: "field" }, [h("label", { text: "Server" }), server]),
      h("div", { class: "field" }, [h("label", { text: "Username" }), username]),
      h("div", { class: "field" }, [h("label", { text: "Password" }), password]),
      submit,
      quickButton,
    ],
  );

  server.addEventListener("change", async () => {
    error.textContent = "";
    try {
      const info = await api("/api/auth/connect", { method: "POST", body: { server: server.value } });
      serverInfo.textContent = `${info.serverName || "Jellyfin"} · ${info.version || ""}`;
      quickButton.style.display = info.quickConnect ? "" : "none";
    } catch (failure) {
      serverInfo.textContent = "Sign in to your Jellyfin server.";
      error.textContent = failure.message || "Could not reach that server.";
    }
  });

  el.append(
    h("div", { class: "login" }, [
      h("div", { class: "login-card" }, [
        h("h1", { text: "MediaFlick Desktop" }),
        serverInfo,
        form,
        quickHost,
        error,
      ]),
    ]),
  );
  if (server.value) server.dispatchEvent(new Event("change"));
}

async function startQuickConnect(serverUrl, host, error) {
  clear(host);
  error.textContent = "";
  let started;
  try {
    started = await api("/api/auth/quickconnect/start", {
      method: "POST",
      body: { server: serverUrl },
    });
  } catch (failure) {
    error.textContent = failure.message || "Quick Connect is unavailable on this server.";
    return;
  }
  host.append(
    h("div", { class: "quick-code" }, [
      "Enter this code in Jellyfin",
      h("strong", { text: started.code }),
    ]),
  );

  const deadline = Date.now() + 5 * 60 * 1000;
  const tick = async () => {
    if (Date.now() > deadline) {
      error.textContent = "Quick Connect timed out. Try again.";
      return;
    }
    try {
      const result = await api("/api/auth/quickconnect/poll", {
        method: "POST",
        body: { server: started.serverUrl, secret: started.secret },
      });
      if (result.authenticated) {
        state.status = result.session;
        navigate("#/");
        return;
      }
    } catch (failure) {
      error.textContent = failure.message || "Quick Connect failed.";
      return;
    }
    setTimeout(tick, 2000);
  };
  setTimeout(tick, 2000);
}

// ------------------------------------------------------------------ startup

async function refreshStatus() {
  try {
    state.status = await api("/api/status");
  } catch (error) {
    state.status = null;
    if (error.status !== 401) toast(error.message, "error");
  }
}

async function render() {
  if (state.cleanup) {
    state.cleanup();
    state.cleanup = null;
  }
  // Status is a local query, so refreshing it every navigation keeps the sync
  // banner and item counts honest without a server round trip.
  await refreshStatus();
  if (!state.status || !state.status.authenticated) {
    loginView();
    return;
  }
  switch (state.route.name) {
    case "login":
      navigate("#/");
      return;
    case "library":
      libraryView();
      return;
    case "item":
      await itemView(state.route.id);
      return;
    default:
      await homeView();
  }
}

window.addEventListener("focus", () => {
  api("/api/sync", { method: "POST" }).catch(() => {});
});

state.route = parseRoute();
render();
pollPlayer();
