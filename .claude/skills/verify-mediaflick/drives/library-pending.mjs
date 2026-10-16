// A movies library whose titles are still arriving shows the loading
// placeholder, not "Nothing to show", even when Hide watched filters its
// grid. A loopback proxy in front of the demo server drops only the Movie
// membership walk, so the library stays pending and its grid empty. Hide
// watched is turned on in Settings, the library opens with the Unwatched
// filter and must keep the placeholder. Once the proxy heals and a sync runs,
// the grid must settle to the API's answer. Then the proxy drops the walk
// again and a requested sync owes it anew: a library walked once is no longer
// pending, so after a reload its grid filtered to a genre nobody has says
// "Nothing to show". Read-only against the demo server.
//   just verify library-pending
const DEMO = "https://demo.jellyfin.org/stable"
const HIDE_WATCHED = { role: "switch", name: "Hide watched by default" }
const UNWATCHED_CHIP = { role: "button", name: "Remove Unwatched filter" }
const CARD = '[data-item-grid-canvas] a[aria-label^="Open details for "]'
const SAMPLES = 10
const SAMPLE_MS = 500
const NO_GENRE = "MediaFlick verify no such genre"

const movieWalk = (url) =>
  url.pathname === "/Items"
  && url.searchParams.has("parentId")
  && url.searchParams.get("IncludeItemTypes") === "Movie"
  && url.searchParams.get("SortBy") === "DateCreated,SortName"

const gridState = (ctx) => ctx.app.evaluate(`document.body.innerText.includes("Nothing to show") ? "empty" : document.querySelector(${JSON.stringify(CARD)}) ? "titles" : document.querySelector('[data-slot="skeleton"]') ? "placeholder" : "blank"`)

async function openByUrl(ctx, path) {
  await ctx.app.evaluate(`history.pushState(null, "", ${JSON.stringify(path)}); dispatchEvent(new PopStateEvent("popstate"))`)
  await ctx.until(async () => (await ctx.pathname()) === path, `the ${path} route`)
}

async function sample(ctx) {
  const states = []
  for (let index = 0; index < SAMPLES; index++) {
    states.push(await gridState(ctx))
    await new Promise((resolve) => setTimeout(resolve, SAMPLE_MS))
  }
  return states
}

export default async function libraryPending(ctx) {
  const report = { checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  const proxy = await ctx.proxy({ upstream: DEMO, fail: movieWalk })
  try {
    await ctx.signIn({ server: proxy.url, username: "demo" })
    const pending = await ctx.until(async () => {
      const status = (await ctx.api("/api/status")).body
      if (status?.bootstrap?.state !== "ready" || status.syncProgress?.active) return null
      const body = (await ctx.api("/api/libraries")).body
      const movies = body?.known ? body.libraries.find((library) => library.kind === "movies") : null
      return movies?.membershipPending && proxy.stats.failed > 0 ? movies : null
    }, "the first sync to finish with the Movie walk failing", 300000)
    report.library = { id: pending.id, name: pending.name }
    expect(pending.browse.filters.includes("watched"), `${pending.name} offers the watched filter`)
    const stored = await ctx.api(`/api/libraries/${pending.id}/items?limit=1&watched=unwatched`)
    expect(stored.status === 200 && stored.body.total === 0, `${pending.name} holds no titles yet (${stored.status}, total ${stored.body?.total})`)

    await openByUrl(ctx, "/settings/viewing")
    await ctx.find(HIDE_WATCHED, { timeout: 20000 })
    if (!(await ctx.state(HIDE_WATCHED)).checked) await ctx.press(HIDE_WATCHED)
    await ctx.press({ role: "button", name: "Save" })
    await ctx.until(() => ctx.readConfig("accounts.json")?.accounts?.some((account) => account.viewing?.hideWatched === true), "accounts.json to record Hide watched")

    await openByUrl(ctx, `/library/${pending.id}`)
    await ctx.find({ role: "heading", name: pending.name }, { timeout: 30000 })
    expect(await ctx.exists(UNWATCHED_CHIP), `${pending.name} opens with the Unwatched filter from Hide watched`)
    report.pendingStates = await sample(ctx)
    expect(report.pendingStates.every((state) => state === "placeholder"), `while ${pending.name} is pending its empty grid shows the placeholder in all ${SAMPLES} samples (${[...new Set(report.pendingStates)].join(", ")})`)
    await ctx.screenshot("01-pending-unwatched")

    proxy.heal()
    await ctx.app.evaluate(`fetch("/api/sync", { method: "POST" }).then((response) => response.status)`)
    const settled = await ctx.until(async () => {
      const body = (await ctx.api("/api/libraries")).body
      const movies = body?.libraries?.find((library) => library.id === pending.id)
      return movies && !movies.membershipPending ? movies : null
    }, `${pending.name} to settle after the proxy heals`, 120000)
    report.settled = { membershipPending: settled.membershipPending }
    const answer = (await ctx.api(`/api/libraries/${pending.id}/items?limit=1&watched=unwatched`)).body?.total
    const expected = answer > 0 ? "titles" : "empty"
    const state = await ctx.until(async () => {
      const now = await gridState(ctx)
      return now === expected ? now : null
    }, `the grid to show ${expected}`, 30000).catch(() => gridState(ctx))
    report.settledState = { api: answer, grid: state }
    expect(state === expected, `after the walk lands the grid shows ${state}, the API counts ${answer} unwatched`)
    await ctx.screenshot("02-settled")

    proxy.resumeFailing()
    const failedBefore = proxy.stats.failed
    await ctx.app.evaluate(`fetch("/api/sync", { method: "POST" }).then((response) => response.status)`)
    const rewalk = await ctx.until(async () => {
      if (proxy.stats.failed <= failedBefore) return null
      const status = (await ctx.api("/api/status")).body
      if (status?.syncProgress?.active) return null
      return (await ctx.api("/api/libraries")).body?.libraries?.find((library) => library.id === pending.id) ?? null
    }, `a requested sync to fail the walk of ${pending.name} again`, 120000)
    report.rewalk = { membershipPending: rewalk.membershipPending, failedWalks: proxy.stats.failed - failedBefore }
    expect(rewalk.membershipPending === false, `${pending.name}, walked once, is not pending while its re-walk fails (${rewalk.membershipPending})`)
    const genre = encodeURIComponent(NO_GENRE)
    const none = await ctx.api(`/api/libraries/${pending.id}/items?limit=1&genre=${genre}`)
    expect(none.status === 200 && none.body.total === 0, `${pending.name} has no title in the genre "${NO_GENRE}" (${none.status}, total ${none.body?.total})`)
    const emptyPath = `/library/${pending.id}?genre=${genre}`
    await ctx.app.evaluate(`history.replaceState(null, "", ${JSON.stringify(emptyPath)}); location.reload()`)
    await ctx.until(async () => (await ctx.pathname()).startsWith(`/library/${pending.id}?genre=`) && (await ctx.app.evaluate("document.readyState")) === "complete", "the filtered library to reload", 30000)
    await ctx.find({ role: "heading", name: pending.name }, { timeout: 30000 })
    report.rewalkStates = await sample(ctx)
    expect(report.rewalkStates.every((state) => state === "empty"), `while the re-walk of ${pending.name} fails its empty filtered grid says Nothing to show in all ${SAMPLES} samples (${[...new Set(report.rewalkStates)].join(", ")})`)
    await ctx.screenshot("03-rewalk-failing-empty")
  } finally {
    ctx.writeEvidence("library-pending.json", report)
  }

  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
