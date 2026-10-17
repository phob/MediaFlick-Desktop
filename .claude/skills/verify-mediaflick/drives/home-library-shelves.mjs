// Home's "Latest in <library>" shelves against Jellyfin's own answers. Home
// settings list one shelf per /UserViews entry, in its order, enabled, under
// Library. Each library with titles gets a shelf on Home whose ids equal the
// query its profile names: /Items/Latest for grouped live libraries, the
// library's grid newest first for box sets, playlists and Live TV, and for
// movies and TV shows libraries the Jellyfin data the cache sorts by (date
// added; a show's newest episode). /latest names the library sort that
// matches the shelf (episode-added for TV shows, added otherwise), and the
// shelf's All link opens the library with it. Then, in the disposable profile's accounts.json: one shelf
// turned off and saved, another toggled and discarded (the file keeps its
// hash), and Restore defaults saved (every library shelf on, in server
// order). Read-only on the server.
//   just verify home-library-shelves
//   just verify home-library-shelves --account https://jellyfin.example.com
import { createHash } from "node:crypto"
import { existsSync, readFileSync } from "node:fs"

const DEMO = "https://demo.jellyfin.org/stable"
const SETTINGS_HOME = { role: "link", name: "Home" }
const SHELF_LIMIT = 24
const DEPTH = 60
const JELLYFIN_PAGE = 1000
const IDS_ONLY = "EnableImages=false&EnableUserData=false"
const PROFILES = {
  movies: { source: "catalog: Movie by date added" },
  shows: { source: "catalog: Series by newest episode added" },
  music: { source: "grouped" },
  musicVideos: { source: "grouped" },
  books: { source: "grouped" },
  homeVideos: { source: "grouped" },
  photos: { source: "grouped" },
  mixed: { source: "grouped" },
  trailers: { source: "grouped" },
  boxSets: { source: "newest", types: ["BoxSet"] },
  playlists: { source: "newest" },
  liveTv: { source: "newest" },
}

const same = (left, right) => JSON.stringify(left) === JSON.stringify(right)
const label = (name) => `Latest in ${name}`

const SETTINGS_ROWS = `[...document.querySelectorAll("[data-home-element-key]")].map((row) => {
  const name = row.querySelector("label")
  return {
    key: row.dataset.homeElementKey,
    label: name?.textContent.trim() ?? null,
    category: name?.nextElementSibling?.textContent.trim() ?? null,
    checked: row.querySelector('button[role="checkbox"]')?.getAttribute("aria-checked") === "true",
  }
})`

const HOME_SHELVES = `[...document.querySelectorAll("main section[aria-labelledby]")].map((section) => ({
  title: section.querySelector("h2")?.textContent.trim() ?? null,
  ids: [...section.querySelectorAll('a[aria-label^="Open details for"]')].map((link) => decodeURIComponent(link.getAttribute("href").replace(/^.*\\/item\\//, ""))),
  viewAll: [...section.querySelectorAll("a")].find((link) => link.textContent.trim() === "All")?.getAttribute("href") ?? null,
}))`

const FIND_CLIENT = `const client = (() => {
  const root = document.getElementById("root")
  const stack = [root?.[Object.keys(root).find((key) => key.startsWith("__reactContainer$"))]]
  while (stack.length) {
    const fiber = stack.pop()
    if (!fiber) continue
    if (fiber.memoizedProps?.client?.getQueryCache) return fiber.memoizedProps.client
    stack.push(fiber.child, fiber.sibling)
  }
  return null
})()`

const HOME_SETTINGS_IDLE = `(() => {
  ${FIND_CLIENT}
  const query = client?.getQueryCache().find({ queryKey: ["home", "settings"], exact: true })
  return Boolean(query?.state.data) && query.state.fetchStatus === "idle" && !query.isStale()
})()`

const LATEST_QUERIES = `(() => {
  ${FIND_CLIENT}
  return client?.getQueryCache().findAll({ queryKey: ["library-latest"] }).map((query) => ({
    id: query.queryKey[1],
    status: query.state.status,
    fetching: query.state.fetchStatus === "fetching",
    items: query.state.data?.items?.length ?? null,
    sort: query.state.data?.sort ?? null,
  })) ?? null
})()`

async function showShelf(ctx, title) {
  await ctx.app.evaluate(`[...document.querySelectorAll("main section[aria-labelledby] h2")].find((h2) => h2.textContent.trim() === ${JSON.stringify(title)})?.closest("section").scrollIntoView({ block: "center" })`)
}

function accountsHash(ctx) {
  const file = ctx.configFile("accounts.json")
  return existsSync(file) ? createHash("sha256").update(readFileSync(file)).digest("hex") : null
}

function storedHome(ctx, userId) {
  return ctx.readConfig("accounts.json")?.accounts?.find((account) => account.userId === userId)?.home ?? null
}

function fileState(ctx, userId) {
  return { sha256: accountsHash(ctx), home: storedHome(ctx, userId) }
}

async function openHomeSettings(ctx) {
  await ctx.park()
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/settings/"), "Settings")
  await ctx.press(SETTINGS_HOME, { nth: 1 })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/home", "the Home settings shelf")
  await ctx.find({ role: "checkbox", name: "Show billboard" }, { timeout: 30000 })
  await ctx.until(() => ctx.app.evaluate(HOME_SETTINGS_IDLE), "the Home settings to load", 30000)
}

async function goHome(ctx) {
  await ctx.park()
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press(SETTINGS_HOME, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
}

async function save(ctx, what) {
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, what)
}

async function pages(ctx, path) {
  const items = []
  for (let offset = 0; ; offset += JELLYFIN_PAGE) {
    const page = await ctx.jellyfin(`${path}&StartIndex=${offset}&Limit=${JELLYFIN_PAGE}`)
    items.push(...page.Items)
    if (items.length >= page.TotalRecordCount || page.Items.length === 0) return items
  }
}

/**
 * Rows `{ id, key }` in the order the shelf must follow, deeper than the shelf
 * so a tie group cut by the limit is whole, with the total that bounds the
 * shelf's length. `key` is null where the order is Jellyfin's own and exact.
 */
async function oracle(ctx, userId, library) {
  const base = `/Items?userId=${userId}&ParentId=${library.id}`
  if (library.kind === "movies") {
    const page = await ctx.jellyfin(`${base}&IncludeItemTypes=Movie&Recursive=true&SortBy=DateCreated&SortOrder=Descending&Fields=DateCreated&Limit=${DEPTH}&${IDS_ONLY}`)
    return {
      query: `${base}&IncludeItemTypes=Movie&Recursive=true&SortBy=DateCreated&SortOrder=Descending`,
      total: page.TotalRecordCount,
      rows: page.Items.map((item) => ({ id: item.Id, key: item.DateCreated, type: item.Type })),
    }
  }
  if (library.kind === "shows") {
    const series = await pages(ctx, `${base}&IncludeItemTypes=Series&Recursive=true&Fields=DateCreated&${IDS_ONLY}`)
    const episodes = await pages(ctx, `${base}&IncludeItemTypes=Episode&Recursive=true&Fields=DateCreated&${IDS_ONLY}`)
    const newest = new Map()
    for (const episode of episodes) {
      if (episode.LocationType === "Virtual" || !episode.SeriesId) continue
      const current = newest.get(episode.SeriesId)
      if (!current || episode.DateCreated > current) newest.set(episode.SeriesId, episode.DateCreated)
    }
    const rows = series
      .map((show) => ({ id: show.Id, key: newest.get(show.Id) ?? show.DateCreated, type: show.Type }))
      .sort((left, right) => (left.key < right.key ? 1 : left.key > right.key ? -1 : 0))
    const contentAdded = await ctx.jellyfin(`${base}&IncludeItemTypes=Series&Recursive=true&SortBy=DateLastContentAdded&SortOrder=Descending&Fields=DateLastContentAdded&Limit=${DEPTH}&${IDS_ONLY}`)
    return {
      query: `max non-virtual Episode DateCreated per Series (else the Series' DateCreated), from ${base}&IncludeItemTypes=Series|Episode&Recursive=true`,
      total: series.length,
      rows,
      episodes: episodes.length,
      dateLastContentAdded: {
        query: `${base}&IncludeItemTypes=Series&Recursive=true&SortBy=DateLastContentAdded&SortOrder=Descending`,
        ...tieAware(contentAdded.Items.slice(0, Math.min(SHELF_LIMIT, series.length)).map((item) => item.Id), rows, series.length),
      },
    }
  }
  if (PROFILES[library.kind].source === "grouped") {
    const query = `/Items/Latest?userId=${userId}&ParentId=${library.id}&Limit=${SHELF_LIMIT}`
    const items = await ctx.jellyfin(`${query}&${IDS_ONLY}`)
    return { query, total: items.length, rows: items.map((item) => ({ id: item.Id, key: null, type: item.Type })) }
  }
  const { types } = PROFILES[library.kind]
  const query = `${base}${types ? `&Recursive=true&IncludeItemTypes=${types.join(",")}` : ""}&SortBy=DateCreated,SortName&SortOrder=Descending`
  const page = await ctx.jellyfin(`${query}&Limit=${SHELF_LIMIT}&${IDS_ONLY}`)
  return { query, total: page.TotalRecordCount, rows: page.Items.map((item) => ({ id: item.Id, key: null, type: item.Type })) }
}

/**
 * The shelf matches when it is as long as the library allows and each of its
 * ids sits where the oracle puts an item with the same sort key. Items whose
 * keys tie may come in either order; where the order is Jellyfin's own
 * (`key` null) the ids must match exactly.
 */
function tieAware(shelfIds, rows, total) {
  const length = Math.min(SHELF_LIMIT, total)
  const expectedIds = rows.slice(0, length).map((row) => row.id)
  const exact = same(shelfIds, expectedIds)
  if (rows.every((row) => row.key === null)) return { match: exact && shelfIds.length === length, exact, ties: [] }
  const keyOf = new Map(rows.map((row) => [row.id, row.key]))
  const shelfKeys = shelfIds.map((id) => keyOf.get(id) ?? null)
  const expectedKeys = rows.slice(0, length).map((row) => row.key)
  const groups = new Map()
  for (const row of rows) groups.set(row.key, [...(groups.get(row.key) ?? []), row.id])
  const ties = [...new Set(expectedKeys)]
    .map((key) => ({ key, ids: groups.get(key) }))
    .filter((group) => group.ids.length > 1)
  const match = shelfIds.length === length
    && new Set(shelfIds).size === shelfIds.length
    && shelfKeys.every((key, index) => key !== null && key === expectedKeys[index])
  return { match, exact, ties }
}

const viewAllHref = (library, sort) => `/library/${encodeURIComponent(library.id)}?sort=${sort}`

function libraryElements(home) {
  return (home?.elements ?? []).filter((element) => element.kind === "library")
}

export default async function homeLibraryShelves(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)
  await ctx.until(async () => (await ctx.api("/api/libraries")).body?.known === true, "the library list", 60000)

  const report = { mode: ctx.account ? `account ${ctx.account.server}` : "demo", checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
    return Boolean(condition)
  }

  const { userId } = (await ctx.api("/api/status")).body
  const views = (await ctx.jellyfin(`/UserViews?userId=${userId}`)).Items.map((view) => ({ id: view.Id.toLowerCase(), name: view.Name, collectionType: view.CollectionType ?? null }))
  const libraries = (await ctx.api("/api/libraries")).body.libraries
  report.userViews = views
  expect(same(libraries.map((library) => library.id), views.map((view) => view.id)), `/api/libraries lists the ${views.length} /UserViews entries in order`)

  report.accountsBeforeSettings = fileState(ctx, userId)
  await openHomeSettings(ctx)
  const rows = await ctx.until(async () => {
    const read = await ctx.app.evaluate(SETTINGS_ROWS)
    return read.filter((row) => row.key.startsWith("library:")).length >= views.length ? read : null
  }, "a library shelf row per library", 30000)
  report.settingsOrder = rows
  const libraryRows = rows.filter((row) => row.key.startsWith("library:"))
  expect(same(libraryRows.map((row) => row.key), views.map((view) => `library:${view.id}`)), `Home settings list one library shelf per /UserViews entry, in its order (${libraryRows.map((row) => row.label).join(", ")})`)
  expect(same(libraryRows.map((row) => row.label), views.map((view) => label(view.name))), "each is labelled Latest in <library>")
  expect(libraryRows.every((row) => row.checked && row.category === "Library"), "each is on by default under Library")
  const firstLibrary = rows.findIndex((row) => row.key.startsWith("library:"))
  const lastBuiltIn = rows.findLastIndex((row) => row.category === "Built-in")
  expect(firstLibrary === lastBuiltIn + 1 && rows.slice(firstLibrary, firstLibrary + views.length).every((row) => row.key.startsWith("library:")), `the library shelves follow the built-in shelves as one block (rows ${firstLibrary}..${firstLibrary + views.length - 1})`)
  await ctx.park()
  await ctx.screenshot("01-home-settings")
  report.accountsAfterSettings = fileState(ctx, userId)
  expect(same(report.accountsAfterSettings.home, report.accountsBeforeSettings.home) && libraryElements(report.accountsAfterSettings.home).length === 0, "opening Home settings writes no library shelf to accounts.json")

  await goHome(ctx)
  report.libraries = []
  for (const library of libraries) {
    report.libraries.push({ id: library.id, name: library.name, kind: library.kind, source: PROFILES[library.kind].source, oracle: await oracle(ctx, userId, library) })
  }
  const settled = await ctx.until(async () => {
    const queries = await ctx.app.evaluate(LATEST_QUERIES)
    const done = libraries.every((library) => queries?.some((query) => query.id === library.id && query.status !== "pending" && !query.fetching))
    return done ? queries : null
  }, "every library shelf to settle", 60000)
  report.shelfQueries = settled
  await ctx.park()
  const shelves = await ctx.app.evaluate(HOME_SHELVES)
  report.homeShelves = shelves.map(({ title }) => title)
  for (const entry of report.libraries) {
    const library = libraries.find((candidate) => candidate.id === entry.id)
    const shelf = shelves.find((candidate) => candidate.title === label(library.name))
    entry.shelfIds = shelf?.ids ?? []
    entry.oracleIds = entry.oracle.rows.slice(0, Math.min(SHELF_LIMIT, entry.oracle.total)).map((row) => row.id)
    entry.oracleTypes = [...new Set(entry.oracle.rows.map((row) => row.type))]
    entry.sort = { expected: library.kind === "shows" ? "episode-added" : "added", returned: settled.find((query) => query.id === library.id)?.sort ?? null }
    expect(entry.sort.returned === entry.sort.expected && library.browse.sorts.includes(entry.sort.returned), `${library.name} (${library.kind}): /latest names sort ${entry.sort.returned}, expected ${entry.sort.expected}, one of the library's sorts`)
    if (entry.oracle.total === 0) {
      entry.match = !shelf
      expect(entry.match, `${library.name} (${library.kind}) holds nothing and has no shelf`)
      continue
    }
    if (!expect(shelf, `${library.name} (${library.kind}) has a "${label(library.name)}" shelf on Home`)) {
      entry.match = false
      continue
    }
    Object.assign(entry, tieAware(entry.shelfIds, entry.oracle.rows, entry.oracle.total))
    const order = `${entry.exact ? "exact order" : `same order up to ${entry.ties.length} tie groups`}; oracle ${entry.oracleIds.length} of ${entry.oracle.total}`
    expect(entry.match, PROFILES[library.kind].source.startsWith("catalog")
      ? `${label(library.name)} lists ${entry.shelfIds.length} ids matching ${entry.source} (${order})`
      : `${label(library.name)} matches its profile's Jellyfin query (${entry.source}): ${entry.shelfIds.length} ids (${order})`)
    entry.viewAll = { expected: { sort: entry.sort.returned, href: viewAllHref(library, entry.sort.returned) }, shown: shelf.viewAll }
    expect(shelf.viewAll === entry.viewAll.expected.href, `${label(library.name)}: All links to the sort /latest named, ${entry.viewAll.expected.href} (${shelf.viewAll})`)
    if (library.kind === "shows") expect(shelf.viewAll === viewAllHref(library, "episode-added"), `${label(library.name)}: a TV shows shelf's All is sort=episode-added`)
    if (library.kind === "shows") {
      ctx.step(`measured: ${library.name} SortBy=DateLastContentAdded ${entry.oracle.dateLastContentAdded.match ? "matches" : "does not match"} the newest-episode order`)
    }
  }
  for (const [index, entry] of report.libraries.filter((candidate) => candidate.shelfIds.length).entries()) {
    await showShelf(ctx, label(entry.name))
    await ctx.screenshot(`02-home-shelf-${String(index + 1).padStart(2, "0")}`)
  }

  const withShelf = report.libraries.filter((entry) => entry.viewAll)
  for (const entry of withShelf) {
    const library = libraries.find((candidate) => candidate.id === entry.id)
    const { href } = entry.viewAll.expected
    // Every shelf's link is named "All", so its href is the only unique handle.
    await ctx.press({ css: `main section a[href="${href}"]` })
    await ctx.until(async () => (await ctx.pathname()) === href, `${label(library.name)}'s All to open ${href}`)
    await ctx.find({ role: "heading", name: library.name })
    entry.viewAll.landed = await ctx.pathname()
    expect(entry.viewAll.landed === href, `${label(library.name)}: All opens ${library.name} sorted by ${entry.viewAll.expected.sort}`)
    await goHome(ctx)
    await ctx.until(async () => (await ctx.app.evaluate(HOME_SHELVES)).some((shelf) => shelf.title === label(library.name)), `${label(library.name)} on Home again`)
  }

  const off = libraries.find((library) => withShelf.some((entry) => entry.id === library.id)) ?? libraries[0]
  const toggled = libraries.find((library) => library.id !== off.id)
  report.disabled = { id: off.id, name: off.name, before: fileState(ctx, userId) }
  await openHomeSettings(ctx)
  await ctx.press({ role: "checkbox", name: `Show ${label(off.name)}` })
  await save(ctx, `${label(off.name)} off to save`)
  report.disabled.after = fileState(ctx, userId)
  const savedOff = libraryElements(report.disabled.after.home)
  expect(same(savedOff, libraries.map((library) => ({ kind: "library", id: library.id, enabled: library.id !== off.id }))), `accounts.json holds every library shelf in server order, ${off.name}'s with enabled:false`)
  await goHome(ctx)
  const remaining = withShelf.filter((entry) => entry.id !== off.id).map((entry) => label(entry.name))
  const afterOff = await ctx.until(async () => {
    const shown = await ctx.app.evaluate(HOME_SHELVES)
    const titles = shown.map(({ title }) => title)
    return !titles.includes(label(off.name)) && remaining.every((title) => titles.includes(title)) ? shown : null
  }, `Home without ${label(off.name)} and with the other library shelves`, 30000).catch(() => ctx.app.evaluate(HOME_SHELVES))
  report.disabled.homeShelves = afterOff.map(({ title }) => title)
  expect(!afterOff.some((shelf) => shelf.title === label(off.name)), `Home no longer shows ${label(off.name)}`)
  expect(withShelf.filter((entry) => entry.id !== off.id).every((entry) => afterOff.some((shelf) => shelf.title === label(entry.name))), "the other library shelves stay")
  await showShelf(ctx, remaining[0] ?? "")
  await ctx.screenshot("03-home-one-off")

  if (toggled) {
    await openHomeSettings(ctx)
    report.discarded = { id: toggled.id, name: toggled.name, before: fileState(ctx, userId) }
    const checkbox = { role: "checkbox", name: `Show ${label(toggled.name)}` }
    const was = (await ctx.state(checkbox)).checked
    await ctx.press(checkbox)
    await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled !== true, "the toggle to make the draft dirty")
    expect((await ctx.state(checkbox)).checked === !was, `the draft toggles ${label(toggled.name)}`)
    await ctx.press({ role: "button", name: "Discard" })
    await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "Discard to clean the draft")
    report.discarded.after = fileState(ctx, userId)
    expect((await ctx.state(checkbox)).checked === was, `Discard returns ${label(toggled.name)} to its saved state`)
    expect(report.discarded.after.sha256 === report.discarded.before.sha256, `accounts.json keeps its SHA-256 through toggle and Discard (${report.discarded.after.sha256})`)
  } else {
    await openHomeSettings(ctx)
    ctx.step("only one library; Discard is not exercised")
  }

  report.reset = { before: fileState(ctx, userId) }
  await ctx.press({ role: "button", name: "Restore defaults" })
  await save(ctx, "the defaults to save")
  report.reset.after = fileState(ctx, userId)
  expect(same(libraryElements(report.reset.after.home), libraries.map((library) => ({ kind: "library", id: library.id, enabled: true }))), "after Restore defaults and Save, accounts.json holds every library shelf enabled, in server order")
  const resetRows = await ctx.app.evaluate(SETTINGS_ROWS)
  report.reset.settingsOrder = resetRows
  expect(resetRows.filter((row) => row.key.startsWith("library:")).every((row) => row.checked), "Home settings show every library shelf on again")
  await ctx.park()
  await ctx.screenshot("04-home-settings-reset")
  await goHome(ctx)
  await ctx.until(async () => (await ctx.app.evaluate(HOME_SHELVES)).some((shelf) => shelf.title === label(off.name)), `${label(off.name)} back on Home`, 30000)
  await showShelf(ctx, label(off.name))
  await ctx.screenshot("05-home-reset")

  ctx.writeEvidence("home-library-shelves.json", report)
  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
