// The detail page's "More like this" rail, read-only, through a loopback proxy
// in front of the server. A movie's rail must list exactly the movies and
// series Jellyfin's own /Items/<id>/Similar answers, and so must a series'
// rail, or the page has no rail when that answer has no titles. Jellyfin
// breaks ties in its similarity score at random, so two calls can order the
// same titles differently: the rail is compared with Jellyfin's answer as a
// set, and its order with the answer the page itself received. A
// rail card opens its title, and when the proxy drops that title's similar
// request the page still renders, without the rail and without an error
// toast. An episode page never asks for similar titles.
//   just verify similar-titles                                   (demo server)
//   just verify similar-titles --account SERVER --signed-out     (your server)
const DEMO = "https://demo.jellyfin.org/stable"
const RAIL = "More like this"
const TITLE_KINDS = new Set(["Movie", "Series"])
const CANDIDATES = 40

const railNames = (ctx) => ctx.app.evaluate(`(() => {
  const heading = [...document.querySelectorAll("h2")].find((node) => node.textContent === ${JSON.stringify(RAIL)})
  if (!heading) return null
  return [...heading.closest("section").querySelectorAll('a[aria-label^="Open details for "]')]
    .map((link) => link.getAttribute("aria-label").slice("Open details for ".length))
})()`)

// The page's own similar requests, as the fetch wrapper installed below saw them.
const similarRequests = (ctx) => ctx.app.evaluate(`window.__similarRequests.map((entry) => ({ ...entry, names: entry.names && [...entry.names] }))`)
const toastCount = (ctx) => ctx.app.evaluate(`document.querySelectorAll("[data-sonner-toast]").length`)
const itemId = async (ctx) => decodeURIComponent((await ctx.pathname()).split("?")[0].slice("/item/".length))

async function openFromSearch(ctx, title) {
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, title)
  await ctx.until(async () => (await ctx.pathname()) === `/library?search=${encodeURIComponent(title)}`, `the ${title} search`)
  await ctx.press({ role: "link", name: `Open details for ${title}` }, { timeout: 30000 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `the ${title} page`)
  await ctx.find({ role: "heading", name: title })
  return itemId(ctx)
}

export default async function similarTitles(ctx) {
  if (ctx.account && !ctx.account.signedOut) throw new Error("similar-titles signs in through a proxy: use --account SERVER --signed-out")
  const report = { server: ctx.account?.server ?? DEMO, checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  let broken = null
  const proxy = await ctx.proxy({
    upstream: report.server,
    fail: (url) => broken !== null && url.pathname.toLowerCase() === `/items/${broken}/similar`,
  })
  const oracle = async (id) => {
    // The app asks for 40 and keeps the first 20 movies and series.
    const { Items } = await ctx.jellyfin(`/Items/${id}/Similar?userId=${report.userId}&Limit=40`)
    return Items.filter((item) => TITLE_KINDS.has(item.Type)).slice(0, 20).map((item) => ({ id: item.Id, name: item.Name, type: item.Type }))
  }
  // The first title by name whose name is unique and whose similar answer
  // holds at least two titles; else the first unique one, to prove the empty case.
  const pick = async (kind) => {
    const { Items } = await ctx.jellyfin(`/Items?userId=${report.userId}&Recursive=true&IncludeItemTypes=${kind}&SortBy=SortName&Limit=${CANDIDATES}&EnableImages=false&Fields=`)
    const unique = Items.filter((item) => Items.filter((other) => other.Name === item.Name).length === 1 && !item.Name.includes("'"))
    for (const item of unique) {
      const similar = await oracle(item.Id)
      if (similar.length >= 2) return { id: item.Id, name: item.Name, similar }
    }
    return { id: unique[0].Id, name: unique[0].Name, similar: await oracle(unique[0].Id) }
  }
  const settledRequest = (label, id) => ctx.until(async () => {
    const entry = (await similarRequests(ctx)).find((request) => request.url.includes(`/item/${id}/similar`) && request.status !== null)
    return entry ?? null
  }, `the ${label} page's similar request to settle`, 30000)
  const railFor = async (label, title) => {
    const request = await settledRequest(label, title.id)
    const expectedNames = title.similar.map((item) => item.name)
    let shown
    if (expectedNames.length) {
      shown = await ctx.until(async () => (await railNames(ctx))?.length ? railNames(ctx) : null, `the ${label} rail`, 30000)
      expect(JSON.stringify([...shown].sort()) === JSON.stringify([...expectedNames].sort()), `the ${label} rail lists exactly Jellyfin's ${expectedNames.length} similar titles`)
      expect(JSON.stringify(shown) === JSON.stringify(request.names), `the ${label} rail keeps the order of the answer the page received`)
    } else {
      shown = await railNames(ctx)
      expect(shown === null, `the ${label} page has no rail when Jellyfin suggests no titles`)
    }
    report[label] = { id: title.id, name: title.name, expected: title.similar, shown, request }
  }
  const scrollToRail = () => ctx.app.evaluate(`[...document.querySelectorAll("h2")].find((node) => node.textContent === ${JSON.stringify(RAIL)})?.scrollIntoView({ block: "center" })`)
  const postersLoaded = `[...[...document.querySelectorAll("h2")].find((node) => node.textContent === ${JSON.stringify(RAIL)}).closest("section").querySelectorAll("img")].slice(0, 4).every((image) => image.complete && image.naturalWidth > 0)`

  try {
    if (ctx.account) {
      await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
      await ctx.quickConnect({ approve: true, timeout: 60000 })
      ctx.revokeAppSessionAfterRun()
    } else {
      await ctx.signIn({ server: proxy.url, username: "demo" })
    }
    await ctx.until(async () => {
      const status = (await ctx.api("/api/status")).body
      return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
    }, "the first sync to finish", 300000)
    report.userId = (await ctx.jellyfin("/Users/Me")).Id
    await ctx.app.evaluate(`(() => {
      window.__similarRequests = []
      const original = window.fetch
      window.fetch = (input, init) => {
        const url = typeof input === "string" ? input : input.url
        if (!url.includes("/similar")) return original(input, init)
        const entry = { url, status: null, names: null }
        window.__similarRequests.push(entry)
        return original(input, init).then(
          async (response) => {
            if (response.ok) entry.names = (await response.clone().json()).items.map((item) => item.name)
            entry.status = response.status
            return response
          },
          (error) => { entry.status = "rejected"; throw error },
        )
      }
    })()`)

    // A movie: the rail equals Jellyfin's own answer.
    const movie = await pick("Movie")
    expect(movie.similar.length >= 2, `Jellyfin suggests ${movie.similar.length} titles for the movie ${movie.name}`)
    expect((await openFromSearch(ctx, movie.name)) === movie.id, `${movie.name} opened from search`)
    await railFor("movie", movie)
    await scrollToRail()
    await ctx.until(postersLoaded, "the first rail posters to load")
    await ctx.snapshot("01-movie-rail")
    await ctx.screenshot("01-movie-rail")

    // A rail card opens its title; that title's similar request fails upstream.
    const target = movie.similar[0]
    broken = target.id.toLowerCase()
    const failedBefore = proxy.stats.failed
    await ctx.press({ role: "link", name: `Open details for ${target.name}` })
    await ctx.until(async () => (await itemId(ctx)) === target.id, `${target.name}'s page from the rail`)
    await ctx.find({ role: "heading", name: target.name })
    const failedRequest = await settledRequest("failing", target.id)
    expect(failedRequest.status >= 500, `the page's similar request for ${target.name} answered ${failedRequest.status}`)
    expect(proxy.stats.failed > failedBefore, `the proxy dropped ${target.name}'s similar request`)
    await ctx.until(`!document.querySelector('[data-slot="skeleton"]')`, `${target.name}'s page to settle`)
    expect(await railNames(ctx) === null, `${target.name}'s page has no rail when its similar request fails`)
    expect(await ctx.exists({ role: "heading", name: "Cast" }) || await ctx.exists({ role: "heading", name: "Details" }), `${target.name}'s page still renders its sections`)
    expect(await toastCount(ctx) === 0, "a failed similar request shows no error toast")
    report.failed = { id: target.id, name: target.name, request: failedRequest, proxyFailed: proxy.stats.failed - failedBefore }
    await ctx.screenshot("02-similar-failed")
    broken = null

    // A series: the rail equals Jellyfin's answer, or is absent when it is empty.
    const series = await pick("Series")
    expect((await openFromSearch(ctx, series.name)) === series.id, `${series.name} opened from search`)
    await railFor("series", series)
    if (series.similar.length) {
      await scrollToRail()
      await ctx.until(postersLoaded, "the series rail posters to load")
    } else {
      await ctx.app.evaluate(`document.querySelector(".content-viewport")?.scrollTo({ top: 1e6 })`)
    }
    await ctx.screenshot("03-series")

    // An episode page never asks.
    await ctx.app.evaluate(`document.querySelector(".content-viewport")?.scrollTo({ top: 0 })`)
    const episodeLink = (await ctx.snapshot("04-series")).match(/link '(\d+\.[^']+)'/)?.[1]
    if (!episodeLink) throw new Error(`no episode link on ${series.name}`)
    const before = (await similarRequests(ctx)).length
    await ctx.press({ role: "link", name: episodeLink })
    await ctx.until(async () => (await itemId(ctx)) !== series.id, "the episode page")
    await ctx.find({ role: "combobox", name: "Streaming quality" }, { timeout: 30000 })
    await ctx.until(`!document.querySelector('[data-slot="skeleton"]')`, "the episode page to settle")
    const requests = await similarRequests(ctx)
    report.episode = { id: await itemId(ctx), link: episodeLink, requests: requests.slice(before) }
    expect(requests.length === before, "an episode page sends no similar request")
    expect(await railNames(ctx) === null, "an episode page has no rail")
    await ctx.screenshot("05-episode")
  } finally {
    ctx.writeEvidence("similar-titles.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.map((check) => check.message).join("; ")}`)
}
