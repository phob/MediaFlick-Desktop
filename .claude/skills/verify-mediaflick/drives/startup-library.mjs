// Where the app starts for a stored startup destination, and that starting
// never rewrites it. Run once per seed from scripts/startup-destination-seed.mjs:
// `movies` (legacy) must open the first movies library, `library` the TV shows
// library it names, and `missing` (an id no library has) Home. The first
// launch, on an empty library.db, is measured and recorded; the assertions
// run on a relaunch of the same profile, once the library list is known.
// Before the relaunch the drive opens the expected page through the sidebar,
// so the app's own last-route save is a no-op, and accounts.json must keep its
// SHA-256 across the relaunch. Then Settings → Viewing must show the resolved
// destination, list Home, every library in server order, Releases and Last
// browsing page, and Save must write {"library":"<id>"} for the picked library.
//   node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs https://demo.jellyfin.org/stable build/verify-seeds/startup-demo
//   just verify startup-library --seed build/verify-seeds/startup-demo/movies
//   node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs https://jellyfin.example.com build/verify-seeds/startup-account
//   just verify startup-library --account https://jellyfin.example.com --seed build/verify-seeds/startup-account/library
import { createHash } from "node:crypto"
import { existsSync, readFileSync } from "node:fs"

const DEMO = "https://demo.jellyfin.org/stable"
const SELECT = { role: "combobox", name: "Startup destination" }
const same = (left, right) => JSON.stringify(left) === JSON.stringify(right)

const DECISION_INPUTS = `(async () => {
  const root = document.getElementById("root")
  const stack = [root?.[Object.keys(root).find((key) => key.startsWith("__reactContainer$"))]]
  let client = null
  while (stack.length && !client) {
    const fiber = stack.pop()
    if (!fiber) continue
    if (fiber.memoizedProps?.client?.getQueryCache) client = fiber.memoizedProps.client
    stack.push(fiber.child, fiber.sibling)
  }
  const query = (key) => client?.getQueryCache().getAll().find((candidate) => candidate.queryKey[0] === key && candidate.queryKey[1] !== "anonymous:anonymous" && candidate.state.data !== undefined)
  const viewing = query("viewing")
  if (!viewing || !query("browsing")) return null
  await new Promise((settled) => requestAnimationFrame(() => requestAnimationFrame(settled)))
  return {
    landing: location.pathname + location.search,
    viewing: viewing.state.data.startupDestination,
    viewingUpdates: viewing.state.dataUpdateCount,
  }
})()`

const PAGE_TEXT = `(() => (document.querySelector("main") ?? document.body).innerText.trim().slice(0, 400))()`

function accountsHash(ctx) {
  const file = ctx.configFile("accounts.json")
  return existsSync(file) ? createHash("sha256").update(readFileSync(file)).digest("hex") : null
}

function storedAccount(ctx, userId) {
  return ctx.readConfig("accounts.json")?.accounts?.find((account) => account.userId === userId) ?? null
}

async function land(ctx, name, userId) {
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
  const decided = await ctx.until(() => ctx.app.evaluate(DECISION_INPUTS), `${name}: the startup inputs in the page`, 60000)
  const libraries = (await ctx.api("/api/libraries")).body
  const record = {
    launch: name,
    landing: decided.landing,
    viewingSeenByPage: decided.viewing,
    viewingFetches: decided.viewingUpdates,
    librariesKnownAtLanding: libraries?.known ?? null,
    viewingApi: (await ctx.api("/api/settings/viewing")).body?.startupDestination,
  }
  record.lastRoute = (await ctx.until(() => {
    const last = storedAccount(ctx, userId)?.browsing?.last
    return last === record.landing ? { last } : null
  }, `${name}: the app saved its landing as the last route`, 10000)).last
  record.pageText = await ctx.app.evaluate(PAGE_TEXT)
  await ctx.screenshot(`${name}-landing`)
  return record
}

async function settled(ctx) {
  return ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    if (status?.syncing || status?.syncProgress?.active) return null
    const body = (await ctx.api("/api/libraries")).body
    return body?.known && !body.libraries.some((library) => library.membershipPending) ? body : null
  }, "the first sync to finish with the library list known", 600000)
}

async function openThroughSidebar(ctx, route) {
  if ((await ctx.pathname()) === route) return
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press(route === "/" ? { role: "link", name: "Home" } : { css: `[data-sidebar="sidebar"] a[href="${route}"]` })
  await ctx.until(async () => (await ctx.pathname()) === route, `the ${route} route`)
  await ctx.park()
}

async function startupOptions(ctx) {
  await ctx.press(SELECT)
  await ctx.find({ role: "option", name: "Last browsing page" })
  const tree = await ctx.snapshot("startup-options")
  await ctx.screenshot("startup-options")
  await ctx.key("Escape", { code: "Escape", keyCode: 27 })
  await ctx.until(async () => !(await ctx.exists({ role: "listbox" })), "the select to close")
  return [...tree.matchAll(/^\s*option '([^']*)'/gm)].map((match) => match[1])
}

export default async function startupLibrary(ctx) {
  const report = { mode: ctx.account ? "account" : "demo", checks: [], launches: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAILED"}: ${message}`)
  }
  const seed = ctx.readConfig("accounts.json")
  const seeded = seed?.accounts?.[0]?.viewing?.startupDestination
  if (seeded === undefined) throw new Error("start with --seed <dir>/<case> from scripts/startup-destination-seed.mjs")
  ctx.writeEvidence("accounts-seed.json", seed)
  report.stored = seeded
  report.hashes = { seed: accountsHash(ctx) }

  try {
    await ctx.signIn({ server: DEMO, username: "demo" })
    const { userId } = (await ctx.api("/api/status")).body
    const first = await land(ctx, "first", userId)
    report.launches.push(first)
    report.hashes.afterFirstLaunch = accountsHash(ctx)
    ctx.writeEvidence("accounts-after-first.json", ctx.readConfig("accounts.json"))
    expect(same(storedAccount(ctx, userId)?.viewing?.startupDestination, seeded), `first launch: accounts.json still stores ${JSON.stringify(seeded)}`)

    const listed = await settled(ctx)
    const views = (await ctx.jellyfin(`/UserViews?userId=${userId}`)).Items
    const libraries = listed.libraries
    const firstMovies = libraries.find((library) => library.kind === "movies")
    const shows = libraries.find((library) => library.kind === "shows")
    expect(same(libraries.map((library) => library.id), views.map((view) => view.Id.toLowerCase())), "/api/libraries lists /UserViews' ids in its order")
    const named = typeof seeded === "object" ? libraries.find((library) => library.id === seeded.library) : null
    const expected = seeded === "movies" ? firstMovies : typeof seeded === "object" ? named : null
    report.case = seeded === "movies" ? "legacy-movies" : named ? "listed-library" : "missing-library"
    report.expected = { library: expected ? { id: expected.id, name: expected.name, kind: expected.kind } : null, landing: expected ? `/library/${expected.id}` : "/" }
    if (first.landing !== report.expected.landing && first.landing.startsWith("/library/")) {
      const opened = await ctx.app.evaluate(PAGE_TEXT)
      report.firstLaunchLibraryPageLater = opened
      ctx.step(`first launch: the library page it opened now reads ${JSON.stringify(opened.slice(0, 120))}`)
      await ctx.screenshot("first-landing-after-sync")
    }

    await openThroughSidebar(ctx, report.expected.landing)
    await ctx.until(() => storedAccount(ctx, userId)?.browsing?.last === report.expected.landing, "the expected page saved as the last route", 10000)
    report.hashes.beforeRelaunch = accountsHash(ctx)
    ctx.writeEvidence("accounts-before-relaunch.json", ctx.readConfig("accounts.json"))
    await ctx.relaunch()
    const second = await land(ctx, "relaunch", userId)
    report.launches.push(second)
    report.hashes.afterRelaunch = accountsHash(ctx)
    ctx.writeEvidence("accounts-after-relaunch.json", ctx.readConfig("accounts.json"))
    expect(second.librariesKnownAtLanding === true, "relaunch: the library list was known when the app chose its landing")
    expect(second.landing === report.expected.landing, `relaunch: ${JSON.stringify(seeded)} landed on ${second.landing} (expected ${report.expected.landing}${expected ? `, ${expected.name}` : ", Home"})`)
    expect(same(second.viewingApi, expected ? { library: expected.id } : "home"), `relaunch: /api/settings/viewing answers ${JSON.stringify(second.viewingApi)}`)
    expect(report.hashes.afterRelaunch === report.hashes.beforeRelaunch, `relaunch: accounts.json kept SHA-256 ${report.hashes.beforeRelaunch}`)
    expect(same(storedAccount(ctx, userId)?.viewing?.startupDestination, seeded), `relaunch: accounts.json still stores ${JSON.stringify(seeded)}`)

    await ctx.press({ role: "link", name: "Settings" })
    await ctx.press({ role: "link", name: "Viewing" })
    await ctx.until(async () => (await ctx.pathname()) === "/settings/viewing", "Viewing settings")
    const shown = (await ctx.state(SELECT, { timeout: 30000 })).value
    report.settings = { shown, options: await startupOptions(ctx) }
    expect(shown === (expected ? expected.name : "Home"), `Settings shows the resolved destination "${shown}"`)
    expect(same(report.settings.options, ["Home", ...libraries.map((library) => library.name), "Releases", "Last browsing page"]), `the options are Home, ${libraries.length} libraries in server order, Releases and Last browsing page (${report.settings.options.join(", ")})`)
    const target = expected?.id === shows.id ? firstMovies : shows
    await ctx.choose({ name: SELECT.name }, target.name)
    expect(same(storedAccount(ctx, userId)?.viewing?.startupDestination, seeded), "choosing a library leaves accounts.json unchanged until Save")
    await ctx.press({ role: "button", name: "Save" })
    await ctx.until(() => same(storedAccount(ctx, userId)?.viewing?.startupDestination, { library: target.id }), `accounts.json to store {"library":"${target.id}"}`)
    await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled, "the save bar to settle")
    report.hashes.afterSave = accountsHash(ctx)
    report.settings.saved = storedAccount(ctx, userId)?.viewing?.startupDestination
    report.settings.api = (await ctx.api("/api/settings/viewing")).body?.startupDestination
    ctx.writeEvidence("accounts-after-save.json", ctx.readConfig("accounts.json"))
    expect(same(report.settings.saved, { library: target.id }), `Save wrote {"library":"${target.id}"} (${target.name})`)
    expect(same(report.settings.api, { library: target.id }), "/api/settings/viewing reads the saved library back")
    expect((await ctx.state(SELECT)).value === target.name, `Settings shows ${target.name} after Save`)
    await ctx.screenshot("saved")
  } finally {
    ctx.writeEvidence("startup-library.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok)
  if (failed.length) throw new Error(`${failed.length} check(s) failed:\n${failed.map((check) => check.message).join("\n")}`)
}
