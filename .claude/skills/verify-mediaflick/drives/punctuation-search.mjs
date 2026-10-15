// A search made only of punctuation (`!!`), typed into the sidebar search as a
// user would, against a word with punctuation (`night!!`): the first shows no
// results, not an error, and the second still finds titles. Read-only
// against the demo server.
//   just verify punctuation-search
export default async function punctuationSearch(ctx) {
  await ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  const report = {}
  for (const term of ["!!", "night!!"]) {
    await ctx.fill({ role: "textbox", name: "Search the library" }, term)
    await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1] ?? "").get("search") === term, `search=${term} in the URL`)
    const outcome = await ctx.until(async () => {
      const text = await ctx.app.evaluate("document.querySelector('main')?.innerText ?? ''")
      if (text.includes("Could not load these items")) return "error"
      if (text.includes("Nothing to show")) return "empty"
      if ((await ctx.count({ css: 'main a[aria-label^="Open details for"]' })) > 0) return "results"
      return null
    }, `the library page to settle for ${term}`, 15000)
    const api = await ctx.api(`/api/items?search=${encodeURIComponent(term)}&limit=5`)
    report[term] = { page: outcome, apiStatus: api.status, apiTotal: api.body?.total ?? null, apiError: api.status === 200 ? null : api.body }
    ctx.step(`${term}: page ${outcome}, /api/items ${api.status} ${JSON.stringify(api.body).slice(0, 160)}`)
    await ctx.snapshot(`search-${term === "!!" ? "punctuation" : "word"}`)
    await ctx.screenshot(`search-${term === "!!" ? "punctuation" : "word"}`)
  }
  ctx.writeEvidence("punctuation-search.json", report)
  ctx.check(report["night!!"].page === "results", "a word with punctuation still finds titles")
  ctx.check(report["!!"].page === "empty" && report["!!"].apiStatus === 200 && report["!!"].apiTotal === 0, "a punctuation-only search shows no results, not an error")
}
