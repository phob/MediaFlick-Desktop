// While the first catalog download runs, each committed catalog page refreshes
// an open movies or TV shows grid, and never an open grid Jellyfin serves
// directly. The app signs in through a loopback proxy that holds every catalog
// page for a few seconds, so the download lasts long enough to watch a live
// grid and then a catalog grid each across two committed pages, counting the
// page's own requests for that grid. Read-only.
//   just verify catalog-refresh --account https://jellyfin.example.com --signed-out
const PAGE_HOLD_MS = 3000
const PAGES_PER_WINDOW = 2
const PAGE_NOTICE = "bootstrapped a library page"
const DOWNLOAD_DONE = "library bootstrap complete"
const CATALOG_KINDS = new Set(["movies", "shows"])

const catalogPage = (url) =>
  url.pathname === "/Items"
  && url.searchParams.get("SortBy") === "DateCreated"
  && url.searchParams.get("EnableImages") === "true"

const committedPages = (ctx) => ctx.appLog().split(PAGE_NOTICE).length - 1

function countGridRequests() {
  if (window.__gridRequests) return
  window.__gridRequests = []
  const original = window.fetch
  window.fetch = function (input, init) {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url
    const match = /\/api\/libraries\/([^/?]+)\/items/.exec(url)
    if (match) window.__gridRequests.push({ library: decodeURIComponent(match[1]), at: Date.now() })
    return original.call(this, input, init)
  }
}

async function openByUrl(ctx, path) {
  await ctx.app.evaluate(`history.pushState(null, "", ${JSON.stringify(path)}); dispatchEvent(new PopStateEvent("popstate"))`)
  await ctx.until(async () => (await ctx.pathname()) === path, `the ${path} route`)
}

async function watch(ctx, library) {
  await openByUrl(ctx, `/library/${library.id}`)
  await ctx.find({ role: "heading", name: library.name }, { timeout: 30000 })
  await ctx.until(`window.__gridRequests.some((request) => request.library === ${JSON.stringify(library.id)})`, `the ${library.name} grid to load`, 30000)
  await new Promise((resolve) => setTimeout(resolve, 500))
  const pagesBefore = committedPages(ctx)
  const requestsBefore = await ctx.app.evaluate(`window.__gridRequests.length`)
  await ctx.until(() => committedPages(ctx) >= pagesBefore + PAGES_PER_WINDOW, `${PAGES_PER_WINDOW} catalog pages to commit with ${library.name} open`, 60000)
  const requests = await ctx.app.evaluate(`window.__gridRequests.slice(${requestsBefore})`)
  return {
    id: library.id,
    name: library.name,
    kind: library.kind,
    pages: committedPages(ctx) - pagesBefore,
    downloadDone: ctx.appLog().includes(DOWNLOAD_DONE),
    refetches: requests.filter((request) => request.library === library.id).length,
  }
}

export default async function catalogRefresh(ctx) {
  if (!ctx.account?.signedOut) throw new Error("catalog-refresh needs --account SERVER --signed-out")
  const report = { checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  const proxy = await ctx.proxy({ upstream: ctx.account.server, delay: (url) => (catalogPage(url) ? PAGE_HOLD_MS : 0) })
  try {
    await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
    await ctx.quickConnect({ approve: true, timeout: 60000 })
    ctx.revokeAppSessionAfterRun()
    await ctx.app.evaluate(`(${countGridRequests.toString()})()`)
    const libraries = await ctx.until(async () => {
      const body = (await ctx.api("/api/libraries")).body
      return body?.known && body.libraries.length > 0 ? body.libraries : null
    }, "the library list", 60000)
    const catalog = libraries.find((library) => CATALOG_KINDS.has(library.kind))
    const live = libraries.find((library) => !CATALOG_KINDS.has(library.kind))
    expect(catalog && live, `the account lists a catalog library (${catalog?.name}) and a live one (${live?.name})`)
    expect(!ctx.appLog().includes(DOWNLOAD_DONE), "the first catalog download is still running")

    report.live = await watch(ctx, live)
    await ctx.screenshot("01-live-grid")
    expect(report.live.pages >= PAGES_PER_WINDOW && !report.live.downloadDone, `${report.live.pages} catalog pages committed with ${live.name} open, during the first download`)
    expect(report.live.refetches === 0, `the live ${live.name} grid did not refetch on catalog commits (${report.live.refetches} requests)`)

    report.catalog = await watch(ctx, catalog)
    await ctx.screenshot("02-catalog-grid")
    expect(report.catalog.pages >= PAGES_PER_WINDOW && !report.catalog.downloadDone, `${report.catalog.pages} catalog pages committed with ${catalog.name} open, during the first download`)
    expect(report.catalog.refetches >= 1, `the catalog ${catalog.name} grid refetched on catalog commits (${report.catalog.refetches} requests)`)
    report.delayedPages = proxy.stats.delayed
  } finally {
    ctx.writeEvidence("catalog-refresh.json", report)
  }

  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
