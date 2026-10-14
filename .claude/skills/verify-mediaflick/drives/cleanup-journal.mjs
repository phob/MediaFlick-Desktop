// Proves a test favorite is removed from the server even when its run never
// finishes. --account only. Favorites the most recently added movie that is
// not a favorite on its detail page, then fails on purpose, so the after-run
// task must take the favorite off again. With MEDIAFLICK_VERIFY_HARD_EXIT=1 it
// instead ends the session process right after the write, as a kill would;
// the favorite then stays listed in build/verify/pending-favorites.json until
// the next --account run reconciles it before launch. Both modes fail by design.
//   just verify cleanup-journal --account https://jellyfin.example.com
//   $env:MEDIAFLICK_VERIFY_HARD_EXIT = "1"; just verify cleanup-journal --account https://jellyfin.example.com
import { readJournal } from "../scripts/favorites.mjs"

const serverFavorite = async (ctx, id) => (await ctx.jellyfin(`/Items/${encodeURIComponent(id)}?userId=${ctx.account.userId}`)).UserData.IsFavorite

export default async function cleanupJournal(ctx) {
  if (!ctx.account || ctx.account.signedOut) throw new Error("cleanup-journal needs --account SERVER")
  await ctx.signIn({})
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)

  const { items } = (await ctx.api(`/api/items?${new URLSearchParams({ kind: "Movie", favorite: "false", sort: "added", limit: 20 })}`)).body
  let movie = null
  for (const item of items.filter((candidate) => /\p{L}{3,}/u.test(candidate.name))) {
    if (!(await serverFavorite(ctx, item.id))) {
      movie = item
      break
    }
  }
  if (!movie) throw new Error("no recently added movie is free of favorites on the server")
  await ctx.unfavoriteAfterRun(movie.id)

  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, movie.name)
  await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1] ?? "").get("search") === movie.name, "the search route")
  await ctx.press({ css: `main a[aria-label^="Open details for"][href$="/item/${encodeURIComponent(movie.id)}"]` }, { nth: 0, timeout: 30000 })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${encodeURIComponent(movie.id)}`, `${movie.name}'s page`)
  await ctx.press({ role: "button", name: "Add to favorites" })
  await ctx.find({ role: "button", name: "Remove from favorites" })
  await ctx.until(async () => (await serverFavorite(ctx, movie.id)) === true, `${movie.name} with IsFavorite=true on the server`)
  await ctx.screenshot("favorited")
  const journal = readJournal()
  ctx.writeEvidence("journal.json", journal)
  ctx.check(journal.some((entry) => entry.itemId === movie.id), `the journal lists ${movie.name} (${movie.id}) while it is a favorite`)

  if (process.env.MEDIAFLICK_VERIFY_HARD_EXIT === "1") {
    ctx.step(`hard exit with ${movie.name} (${movie.id}) favorited; no after-run task runs`)
    process.exit(1)
  }
  throw new Error(`failing on purpose with ${movie.name} (${movie.id}) favorited; the after-run task must remove it`)
}
