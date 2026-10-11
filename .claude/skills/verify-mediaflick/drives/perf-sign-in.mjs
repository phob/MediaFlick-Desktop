// Times sign-in to Home against Jellyfin's public demo server, read-only.
// metrics.json holds page-clock milliseconds from the real pointerdown on
// Sign in to each milestone, plus every app request CDP saw with its status
// and duration. Summarise several runs with scripts/perf-summary.mjs.
const DEMO = "https://demo.jellyfin.org/stable"
const CARD = 'a[aria-label^="Open details for"]'

export default async function perfSignIn(ctx) {
  const metrics = { milestones: null, requests: [] }
  try {
    await run(ctx, metrics)
  } finally {
    ctx.writeEvidence("metrics.json", metrics)
  }
}

async function run(ctx, metrics) {
  const seen = new Map()
  await ctx.app.send("Network.enable")
  ctx.app.on("Network.requestWillBeSent", (p) => {
    if (!seen.has(p.requestId)) seen.set(p.requestId, { url: p.request.url, method: p.request.method, start: p.timestamp })
  })
  ctx.app.on("Network.responseReceived", (p) => {
    const r = seen.get(p.requestId)
    if (r) r.status = p.response.status
  })
  ctx.app.on("Network.loadingFinished", (p) => {
    const r = seen.get(p.requestId)
    if (r) r.end = p.timestamp
  })
  ctx.app.on("Network.loadingFailed", (p) => {
    const r = seen.get(p.requestId)
    if (r) Object.assign(r, { end: p.timestamp, failed: p.errorText })
  })

  await ctx.app.evaluate(`(() => {
    const a = (window.__perf = { clickAt: null, hits: {} })
    addEventListener("pointerdown", () => { a.clickAt ??= performance.now() }, true)
    const viewportImagesDone = () => {
      const imgs = [...document.querySelectorAll("main img")].filter((i) => { const r = i.getBoundingClientRect(); return r.width > 0 && r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth })
      return imgs.length > 0 && imgs.every((i) => i.complete)
    }
    const conds = {
      sidebar: () => [...document.querySelectorAll("a")].some((l) => l.textContent.trim() === "Settings"),
      firstCard: () => location.pathname === "/" && document.querySelectorAll(${JSON.stringify(CARD)}).length > 0,
      viewportImages: () => location.pathname === "/" && viewportImagesDone(),
    }
    const tick = () => {
      if (a.clickAt !== null) for (const [k, f] of Object.entries(conds)) if (a.hits[k] === undefined && f()) a.hits[k] = Math.round(performance.now() - a.clickAt)
      if (Object.keys(a.hits).length < Object.keys(conds).length) requestAnimationFrame(tick)
    }
    requestAnimationFrame(tick)
    return true
  })()`)

  await ctx.find({ role: "button", name: "Sign in" }, { timeout: 30000 })
  await ctx.fill({ role: "textbox", name: "Server" }, DEMO)
  await ctx.fill({ role: "textbox", name: "Username" }, "demo")
  await ctx.press({ role: "button", name: "Sign in" })
  await ctx.until(`Object.keys(window.__perf.hits).length === 3`, "sign-in milestones", 60000)
  await ctx.sleep(3000)

  metrics.milestones = await ctx.app.evaluate(`window.__perf.hits`)
  const app = [...seen.values()].filter((r) => r.url.startsWith("mediaflick-desktop://app/"))
  const login = app.find((r) => r.url.endsWith("/api/auth/login"))
  metrics.requests = app
    .filter((r) => r.start >= login.start)
    .map((r) => ({
      method: r.method,
      path: new URL(r.url).pathname,
      atMs: Math.round((r.start - login.start) * 1000),
      status: r.status ?? null,
      failed: r.failed ?? null,
      ms: r.end === undefined ? null : Math.round((r.end - r.start) * 1000),
    }))
  const errors = metrics.requests.filter((r) => r.failed || r.status === null || r.status >= 400)
  ctx.step(`milestones ${JSON.stringify(metrics.milestones)} requests=${metrics.requests.length} errors=${errors.length}`)
  await ctx.screenshot("home")
}
