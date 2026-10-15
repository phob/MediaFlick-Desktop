// Every library in the sidebar, the way a user browses it. The sidebar lists
// Jellyfin's /UserViews names in order. Each entry opens /library/<id> with its
// grid, a total equal to /api/libraries/<id>/items, exactly the sorts and
// filters its `browse` capabilities list and the A–Z rail only where
// `letterIndex` is true. A movies or TV shows library takes a sort and a filter
// whose total matches the API; a live library offers its sorts and nothing
// else. The first card opens its page, whose back link names the library and
// returns to the same URL and filters; saving that same last route again
// leaves accounts.json untouched. In a movies or TV shows library a title
// opened from a card's hover preview also names the library. After a reload
// without navigation state, with /api/libraries held, its page already reads
// "Back to library" and leads to the library, with genre chips on it; once
// the list loads it names the library. Read-only.
//
// Started with --seed holding an accounts.json whose account remembers legacy
// `Movie` and `Series` pages, it instead proves the app starts, keeps those
// pages through navigation, and drops them with the first filter change on a
// movies library, saving accounts.json before and after.
//   just verify library-browse
//   just verify library-browse --account https://jellyfin.example.com
//   node .claude/skills/verify-mediaflick/scripts/legacy-browsing-seed.mjs https://jellyfin.example.com build/verify-seeds/legacy-browsing
//   just verify library-browse --account https://jellyfin.example.com --seed build/verify-seeds/legacy-browsing
import { createHash } from "node:crypto"
import { readFileSync } from "node:fs"

const DEMO = "https://demo.jellyfin.org/stable"
const SORT_IDS = {
  Name: "name",
  "Release date": "year",
  "Date added": "added",
  "Date show added": "added",
  "Date episode added": "episode-added",
  Rating: "rating",
  "Date played": "played",
  "Play count": "plays",
  Runtime: "runtime",
  Random: "random",
}
const FILTER_IDS = [
  ["Genre", "genre"],
  ["Release decade", "decade"],
  ["Watch status", "watched"],
  ["Parental rating", "parental"],
  ["Series status", "seriesStatus"],
  ["Favorites", "favorite"],
]
const CARD = 'a[aria-label^="Open details for "]'
const PREVIEW_LINK = '.preview-panel a[aria-label^="Open details for "]'
// Observes the UI's own browsing saves, so the drive knows a save was made
// before it compares accounts.json. It never sends one.
const SAVE_OBSERVER = `(() => {
  if (window.__browsingSaves) return
  window.__browsingSaves = []
  const original = window.fetch
  window.fetch = async function (input, init) {
    const response = await original.apply(this, arguments)
    const url = input instanceof Request ? input.url : String(input)
    if (new URL(url, location.href).pathname === "/api/settings/browsing" && init?.method === "PATCH") {
      window.__browsingSaves.push({ ...JSON.parse(init.body), status: response.status })
    }
    return response
  }
})()`
// Holds the page's /api/libraries requests while sessionStorage asks for it,
// so one reload can be checked with the library list deterministically
// unavailable. __releaseLibraries() lets the held requests through.
const LIBRARIES_HOLD = `(() => {
  if (window.__libraryHolds) return
  window.__libraryHolds = []
  window.__releaseLibraries = () => {
    sessionStorage.removeItem("mediaflickVerifyHoldLibraries")
    window.__libraryHolds.splice(0).forEach((release) => release())
  }
  const original = window.fetch
  window.fetch = async function (input, init) {
    const url = input instanceof Request ? input.url : String(input)
    if (new URL(url, location.href).pathname === "/api/libraries" && sessionStorage.getItem("mediaflickVerifyHoldLibraries")) {
      window.__heldLibraries = (window.__heldLibraries ?? 0) + 1
      await new Promise((release) => window.__libraryHolds.push(release))
    }
    return original.apply(this, arguments)
  }
})()`
const sameList = (left, right) => JSON.stringify(left) === JSON.stringify(right)
const slug = (text) => text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "library"

const shownTotal = (ctx) => ctx.app.evaluate(`(() => {
  if (document.body.innerText.includes("Nothing to show")) return 0
  const match = document.body.innerText.match(/^([\\d,]+) items?$/m)
  return match ? Number(match[1].replace(/,/g, "")) : null
})()`)

const sidebarLibraries = (ctx) => ctx.app.evaluate(`(() => {
  const group = [...document.querySelectorAll('[data-sidebar="group"]')]
    .find((candidate) => candidate.querySelector('[data-sidebar="group-label"]')?.textContent === "Libraries")
  if (!group) return null
  return [...group.querySelectorAll('a[href^="/library/"]')].map((link) => ({
    id: decodeURIComponent(link.getAttribute("href").slice("/library/".length)),
    name: link.textContent.trim(),
    active: link.getAttribute("data-active") === "true",
  }))
})()`)

async function offeredSorts(ctx) {
  await ctx.press({ role: "combobox", name: "Sort by" })
  const listing = await ctx.snapshot("sort-options")
  await ctx.key("Escape")
  await ctx.until(async () => (await ctx.count({ role: "listbox" })) === 0, "the sort list to close", 3000)
  return [...listing.matchAll(/option 'Sort: ([^']*)'/g)].map((match) => SORT_IDS[match[1]] ?? `unknown:${match[1]}`)
}

async function offeredFilters(ctx) {
  if (!(await ctx.exists({ css: 'button[aria-label^="Filters"]' }))) return []
  await ctx.press({ css: 'button[aria-label^="Filters"]' })
  await ctx.find({ role: "menu" })
  const menu = await ctx.snapshot("filter-options")
  await ctx.key("Escape")
  await ctx.until(async () => (await ctx.count({ role: "menu" })) === 0, "the filter menu to close", 3000)
  const items = [...menu.matchAll(/menuitem '([^']*)'/g)].map((match) => match[1])
  return FILTER_IDS.filter(([label]) => items.some((item) => item.startsWith(label))).map(([, id]) => id)
}

async function openLibrary(ctx, library) {
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ css: `[data-sidebar="sidebar"] a[href="/library/${library.id}"]` })
  await ctx.until(async () => (await ctx.pathname()).startsWith(`/library/${library.id}`), `the ${library.name} library route`)
  await ctx.park()
  await ctx.find({ role: "heading", name: library.name })
  return waitForTotal(ctx, `${library.name}'s total`)
}

async function waitForTotal(ctx, label) {
  let previous = null
  const settled = await ctx.until(async () => {
    const total = await shownTotal(ctx)
    const same = total !== null && total === previous
    previous = total
    return same ? { total } : null
  }, label, 60000)
  return settled.total
}

async function apiTotal(ctx, library, query = "") {
  const response = await ctx.api(`/api/libraries/${library.id}/items?limit=1${query}`)
  return response.status === 200 ? response.body.total : `status ${response.status}`
}

function accountsFingerprint(ctx) {
  const stat = ctx.configStat("accounts.json")
  if (!stat) return null
  return { mtimeMs: stat.mtimeMs, sha256: createHash("sha256").update(readFileSync(ctx.configFile("accounts.json"))).digest("hex") }
}

const backLinkHref = (ctx, back) => ctx.app.evaluate(`[...document.querySelectorAll("a")].find((link) => link.textContent.trim() === ${JSON.stringify(back)})?.getAttribute("href") ?? null`)

async function openFirstCardAndReturn(ctx, expect, report, library, label, total) {
  if (!(await ctx.exists({ css: CARD }))) {
    expect(total === 0, `${library.name} ${label}: a card to open (the grid shows ${total})`)
    return { skipped: "empty grid" }
  }
  const before = await ctx.pathname()
  await ctx.until(() => ctx.readConfig("accounts.json")?.accounts?.some((account) => account.browsing?.last === before), `the last route ${before} saved`, 10000)
  const savesBefore = await ctx.app.evaluate("window.__browsingSaves.length")
  const fileBefore = accountsFingerprint(ctx)
  const chipsBefore = await ctx.app.evaluate(`document.querySelector('[aria-label="Active filters"]')?.innerText ?? ""`)
  const sortBefore = await ctx.app.evaluate(`document.querySelector('[aria-label="Sort by"]')?.innerText ?? ""`)
  const name = await ctx.app.evaluate(`document.querySelector(${JSON.stringify(CARD)}).getAttribute("aria-label").slice("Open details for ".length)`)
  await ctx.press({ css: CARD }, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `${label}: ${name}'s page`)
  const back = `Back to ${library.name}`
  await ctx.find({ role: "link", name: back }, { timeout: 20000 })
  await ctx.screenshot(`${slug(library.name)}-${label}-detail`)
  await ctx.press({ role: "link", name: back })
  await ctx.until(async () => (await ctx.pathname()) === before, `${label}: back to ${before}`)
  await ctx.find({ role: "heading", name: library.name })
  const resaved = await ctx.until(() => ctx.app.evaluate(`window.__browsingSaves.slice(${savesBefore}).find((save) => save.page === "last" && save.route === ${JSON.stringify(before)} && save.status === 200) ?? null`), `${label}: the UI to save the same last route again`, 10000)
  const fileAfter = accountsFingerprint(ctx)
  const unchanged = fileBefore?.sha256 === fileAfter?.sha256 && fileBefore?.mtimeMs === fileAfter?.mtimeMs
  report.accountsFile.push({ library: library.name, route: before, save: resaved, before: fileBefore, after: fileAfter, unchanged })
  expect(unchanged, `${library.name} ${label}: saving the same last route again leaves accounts.json untouched (mtime ${fileBefore?.mtimeMs} -> ${fileAfter?.mtimeMs})`)
  const chipsAfter = await ctx.app.evaluate(`document.querySelector('[aria-label="Active filters"]')?.innerText ?? ""`)
  const sortAfter = await ctx.app.evaluate(`document.querySelector('[aria-label="Sort by"]')?.innerText ?? ""`)
  expect(chipsAfter === chipsBefore && sortAfter === sortBefore, `${library.name} ${label}: Back restores the sort (${sortAfter}) and filters (${chipsAfter || "none"})`)
  return { opened: name, url: before, sort: sortAfter, filters: chipsAfter }
}

async function previewAndReload(ctx, expect, library) {
  const back = `Back to ${library.name}`
  const target = `/library/${library.id}`
  const result = {}
  await ctx.park()
  await ctx.until(`!document.querySelector(".preview-panel")`, "no card preview open", 10000)
  await ctx.hover({ css: CARD }, { nth: 0 })
  await ctx.find({ css: PREVIEW_LINK }, { timeout: 10000 })
  result.opened = await ctx.app.evaluate(`document.querySelector(${JSON.stringify(PREVIEW_LINK)}).getAttribute("aria-label").slice("Open details for ".length)`)
  await ctx.press({ css: PREVIEW_LINK })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `${result.opened}'s page from its card preview`)
  result.item = await ctx.pathname()
  result.previewLabel = await ctx.find({ role: "link", name: back }, { timeout: 20000 }).then(() => back, () => null)
  result.previewHref = await backLinkHref(ctx, back)
  expect(result.previewLabel === back && result.previewHref?.startsWith(target), `${library.name}: a title opened from its card preview reads ${back} (${result.previewLabel}, ${result.previewHref})`)
  await ctx.screenshot(`${slug(library.name)}-preview-detail`)

  await ctx.app.evaluate("sessionStorage.setItem('mediaflickVerifyHoldLibraries', '1'); history.replaceState(null, '', location.pathname); location.reload()")
  await ctx.until(async () => (await ctx.pathname()) === result.item && (await ctx.app.evaluate("document.readyState")) === "complete", "the title page to reload", 30000)
  await ctx.until("window.__heldLibraries >= 1", "the reloaded page to wait on a held /api/libraries", 30000)
  result.heldLabel = await ctx.find({ role: "link", name: "Back to library" }, { timeout: 15000 }).then(() => "Back to library", () => null)
  result.heldHref = await backLinkHref(ctx, "Back to library")
  result.heldGenreLinks = await ctx.app.evaluate(`[...document.querySelectorAll('a[href*="genre="]')].map((link) => link.getAttribute("href"))`)
  result.heldBackLinks = await ctx.app.evaluate(`[...document.querySelectorAll("a")].map((link) => link.textContent.trim()).filter((text) => text.startsWith("Back to"))`)
  expect(
    result.heldLabel === "Back to library" && result.heldHref === target && (await ctx.app.evaluate("window.__heldLibraries >= 1 && window.__libraryHolds.length >= 1")),
    `${library.name}: while /api/libraries is held after a reload, the title page reads Back to library and leads to ${target} (${JSON.stringify(result.heldBackLinks)}, ${result.heldHref})`,
  )
  expect(
    result.heldGenreLinks.length === 0 || result.heldGenreLinks.every((href) => href.startsWith(`${target}?genre=`)),
    `${library.name}: while /api/libraries is held, the genre chips already filter ${target} (${result.heldGenreLinks.join(", ") || "no genres"})`,
  )
  await ctx.screenshot(`${slug(library.name)}-reloaded-held-detail`)
  await ctx.app.evaluate("window.__releaseLibraries()")
  result.reloadedLabel = await ctx.find({ role: "link", name: back }, { timeout: 30000 }).then(() => back, () => null)
  result.reloadedHref = await backLinkHref(ctx, back)
  expect(result.reloadedLabel === back && result.reloadedHref === target, `${library.name}: once the list loads, the reloaded title page reads ${back} and leads to ${target} (${result.reloadedLabel}, ${result.reloadedHref})`)
  result.genreLinks = await ctx.app.evaluate(`[...document.querySelectorAll('a[href*="genre="]')].map((link) => link.getAttribute("href"))`)
  expect(result.genreLinks.length === 0 || result.genreLinks.every((href) => href.startsWith(`${target}?genre=`)), `${library.name}: the reloaded page's genre chips filter ${target} (${result.genreLinks.join(", ") || "no genres"})`)
  await ctx.screenshot(`${slug(library.name)}-reloaded-detail`)
  await ctx.press({ role: "link", name: back })
  await ctx.until(async () => (await ctx.pathname()).startsWith(target), `back to ${target} after the reload`)
  await ctx.find({ role: "heading", name: library.name })
  return result
}

async function catalogCase(ctx, expect, library) {
  const unwatched = await apiTotal(ctx, library, "&sort=year&watched=unwatched")
  const [label, watched] = unwatched > 0 ? ["Unwatched", "unwatched"] : ["Watched", "watched"]
  const result = { filter: watched }
  await ctx.choose({ name: "Sort by" }, "Sort: Release date")
  await ctx.until(async () => /[?&]sort=year\b/.test(await ctx.pathname()), "sort=year in the URL")
  await ctx.press({ css: 'button[aria-label^="Filters"]' })
  await ctx.find({ role: "menu" })
  const menu = await ctx.snapshot(`${slug(library.name)}-filters`)
  const watch = [...menu.matchAll(/menuitem '(Watch status[^']*)'/g)].map((match) => match[1])[0]
  await ctx.press({ role: "menuitem", name: watch })
  await ctx.press({ role: "menuitemradio", name: label })
  await ctx.until(async () => (await ctx.pathname()).includes(`watched=${watched}`), `watched=${watched} in the URL`)
  await ctx.until(async () => (await ctx.count({ role: "menu" })) === 0, "the menu to close", 3000)
  result.url = await ctx.pathname()
  result.total = await waitForTotal(ctx, `${library.name} sorted and filtered`)
  result.api = await apiTotal(ctx, library, `&sort=year&watched=${watched}`)
  expect(result.total === result.api, `${library.name}: Release date + ${label} shows ${result.total}, the API's total is ${result.api}`)
  await ctx.screenshot(`${slug(library.name)}-filtered`)
  return result
}

async function legacyRun(ctx, expect, report, seeded) {
  ctx.writeEvidence("accounts-before.json", seeded)
  const legacyPages = () => (ctx.readConfig("accounts.json")?.accounts ?? []).flatMap((account) => Object.keys(account.browsing ?? {}).filter((page) => page === "Movie" || page === "Series"))
  expect(legacyPages().includes("Movie"), "the app started with the seeded legacy Movie page in accounts.json")
  const { libraries } = await ctx.until(async () => {
    const response = await ctx.api("/api/libraries")
    return response.body?.known ? response.body : null
  }, "the library list", 120000)
  const movies = libraries.find((library) => library.kind === "movies")
  expect(movies, "the account has a movies library")
  await openLibrary(ctx, movies)
  await ctx.until(async () => ctx.readConfig("accounts.json")?.accounts?.some((account) => account.browsing?.last?.startsWith(`/library/${movies.id}`)), "the last route saved as the library page", 10000)
  expect(legacyPages().includes("Movie"), "navigating saved the last route and kept the legacy Movie page")
  report.beforeFilter = ctx.readConfig("accounts.json")
  await ctx.choose({ name: "Sort by" }, "Sort: Release date")
  await ctx.until(async () => ctx.readConfig("accounts.json")?.accounts?.some((account) => account.browsing?.[`library:${movies.id}`]), `library:${movies.id} saved`, 10000)
  const after = ctx.readConfig("accounts.json")
  ctx.writeEvidence("accounts-after.json", after)
  expect(legacyPages().length === 0, "the first filter change on a library dropped the legacy Movie and Series pages")
  expect(after.version === 1, "accounts.json stays at version 1")
  await ctx.screenshot("legacy-after-filter")
}

export default async function libraryBrowse(ctx) {
  const report = { mode: ctx.account ? "account" : "demo", checks: [], libraries: [], accountsFile: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAILED"}: ${message}`)
  }
  const seeded = ctx.readConfig("accounts.json")
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.app.send("Page.addScriptToEvaluateOnNewDocument", { source: SAVE_OBSERVER })
  await ctx.app.evaluate(SAVE_OBSERVER)
  await ctx.app.send("Page.addScriptToEvaluateOnNewDocument", { source: LIBRARIES_HOLD })

  try {
    if (seeded?.accounts?.some((account) => account.browsing?.Movie)) {
      report.mode = `${report.mode}-legacy-seed`
      await legacyRun(ctx, expect, report, seeded)
      return
    }

    const { userId } = (await ctx.api("/api/status")).body
    const listed = await ctx.until(async () => {
      const status = (await ctx.api("/api/status")).body
      if (status?.syncing || status?.syncProgress?.active) return null
      const body = (await ctx.api("/api/libraries")).body
      return body?.known && !body.libraries.some((library) => library.membershipPending) ? body : null
    }, "the first sync to finish with every membership read", 600000)
    const views = (await ctx.jellyfin(`/UserViews?userId=${userId}`)).Items
    const sidebar = await ctx.until(async () => {
      const links = await sidebarLibraries(ctx)
      return links?.length ? links : null
    }, "the sidebar's Libraries group")
    report.userViews = views.map((view) => ({ id: view.Id, name: view.Name, collectionType: view.CollectionType ?? null }))
    report.sidebar = sidebar
    expect(sameList(sidebar.map((entry) => entry.name), views.map((view) => view.Name)), `the sidebar lists ${views.length} libraries with /UserViews' names in its order`)
    expect(sameList(sidebar.map((entry) => entry.id), views.map((view) => view.Id.toLowerCase())), "each sidebar entry links to its /UserViews id")

    for (const entry of sidebar) {
      const library = listed.libraries.find((candidate) => candidate.id === entry.id)
      const record = { id: entry.id, name: entry.name, kind: library?.kind, browse: library?.browse }
      report.libraries.push(record)
      if (!library) {
        expect(false, `${entry.name}: in /api/libraries`)
        continue
      }
      ctx.step(`library ${library.name} (${library.kind})`)
      record.total = await openLibrary(ctx, library)
      record.cards = await ctx.count({ css: CARD })
      record.api = await apiTotal(ctx, library)
      expect(record.total === record.api, `${library.name}: the grid shows ${record.total}, /api/libraries/<id>/items says ${record.api}`)
      expect((await sidebarLibraries(ctx)).find((link) => link.id === library.id)?.active, `${library.name}: its sidebar entry is marked active`)
      await ctx.screenshot(`library-${String(report.libraries.length).padStart(2, "0")}-${slug(library.name)}`)

      record.sorts = await offeredSorts(ctx)
      expect(sameList(record.sorts, library.browse.sorts), `${library.name}: offers the sorts ${library.browse.sorts.join(", ")} (shown ${record.sorts.join(", ")})`)
      record.filters = await offeredFilters(ctx)
      expect(sameList(record.filters, library.browse.filters), `${library.name}: offers the filters ${library.browse.filters.join(", ") || "none"} (shown ${record.filters.join(", ") || "none"})`)
      record.rail = await ctx.exists({ css: '[aria-label="Jump to letter"]' })
      expect(record.rail === (library.browse.letterIndex && record.total > 0), `${library.name}: the A–Z rail ${record.rail ? "shows" : "is absent"} (letterIndex ${library.browse.letterIndex})`)
      const shuffle = await ctx.exists({ role: "button", name: "Shuffle again" })
      expect(!shuffle, `${library.name}: no shuffle control on the default sort`)

      if (library.kind === "movies" || library.kind === "shows") {
        record.filtered = await catalogCase(ctx, expect, library)
      } else {
        expect(!library.browse.sorts.includes("random") && !library.browse.letterIndex, `${library.name}: a live library offers no random sort and no letters`)
      }
      record.back = await openFirstCardAndReturn(ctx, expect, report, library, record.filtered ? "filtered" : "plain", record.filtered?.total ?? record.total)
      if (record.filtered && (await ctx.exists({ css: CARD }))) record.preview = await previewAndReload(ctx, expect, library)
    }
  } finally {
    ctx.writeEvidence("library-browse.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok)
  if (failed.length) throw new Error(`${failed.length} check(s) failed:\n${failed.map((check) => check.message).join("\n")}`)
}
