// Where the app starts for a stored startup destination, and that neither
// starting nor saving another setting rewrites it. Run once per seed from
// scripts/startup-destination-seed.mjs: `movies` (legacy) must open the first
// movies library, `library` the TV shows library it names, and `missing` (an
// id no library has) Home.
//   node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs https://demo.jellyfin.org/stable build/verify-seeds/startup-demo
//   just verify startup-library --seed build/verify-seeds/startup-demo/movies
//   node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs https://jellyfin.example.com build/verify-seeds/startup-account
//   just verify startup-library --account https://jellyfin.example.com --seed build/verify-seeds/startup-account/library
import { createHash } from "node:crypto"
import { existsSync, readFileSync } from "node:fs"

const DEMO = "https://demo.jellyfin.org/stable"
const SELECT = { role: "combobox", name: "Startup destination" }
const same = (left, right) => JSON.stringify(left) === JSON.stringify(right)

const FIND_QUERY = `const query = (() => {
  const root = document.getElementById("root")
  const stack = [root?.[Object.keys(root).find((key) => key.startsWith("__reactContainer$"))]]
  let client = null
  while (stack.length && !client) {
    const fiber = stack.pop()
    if (!fiber) continue
    if (fiber.memoizedProps?.client?.getQueryCache) client = fiber.memoizedProps.client
    stack.push(fiber.child, fiber.sibling)
  }
  return (key) => client?.getQueryCache().getAll().find((candidate) => candidate.queryKey[0] === key && candidate.queryKey[1] !== "anonymous:anonymous" && candidate.state.data !== undefined)
})()`

const PAGE_VIEWING = `(() => {
  ${FIND_QUERY}
  const viewing = query("viewing")
  return viewing ? {
    route: location.pathname + location.search,
    viewing: viewing.state.data.startupDestination,
    viewingUpdates: viewing.state.dataUpdateCount,
  } : null
})()`

const DECISION_INPUTS = `(async () => {
  ${FIND_QUERY}
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
    viewingAfterLanding: decided.viewing,
    viewingUpdatesAfterLanding: decided.viewingUpdates,
    librariesKnownAfterLanding: libraries?.known ?? null,
    viewingApi: (await ctx.api("/api/settings/viewing")).body?.startupDestination,
  }
  record.lastRoute = (await ctx.until(() => {
    const last = storedAccount(ctx, userId)?.browsing?.last
    return last === record.landing ? { last } : null
  }, `${name}: the app saved its landing as the last route`, 10000)).last
  record.pageText = await ctx.app.evaluate(PAGE_TEXT)
  await ctx.screenshot(`${name}-landing`)
  await new Promise((settle) => setTimeout(settle, 2000))
  record.viewingUpdatesSettled = (await ctx.app.evaluate(PAGE_VIEWING))?.viewingUpdates
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
  await ctx.press({ css: `[data-sidebar="content"] a[href="${route}"]` })
  await ctx.until(async () => (await ctx.pathname()) === route, `the ${route} route`)
  await ctx.park()
}

async function openSettings(ctx, page) {
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: page })
  await ctx.until(async () => (await ctx.pathname()) === `/settings/${page.toLowerCase()}`, `${page} settings`)
}

async function savePreviewDelay(ctx, userId, label) {
  await openSettings(ctx, "Appearance")
  const field = { role: "spinbutton", name: "Card preview delay" }
  await ctx.until(async () => !(await ctx.state(field)).disabled, `${label}: Card preview delay to load`, 30000)
  const before = storedAccount(ctx, userId)?.viewing ?? {}
  const delay = before.previewDelayMs === 600 ? 650 : 600
  const record = {
    pageViewing: (await ctx.app.evaluate(PAGE_VIEWING))?.viewing,
    librariesKnownBefore: (await ctx.api("/api/libraries")).body?.known ?? null,
    delay,
  }
  await ctx.fill(field, String(delay))
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => storedAccount(ctx, userId)?.viewing?.previewDelayMs === delay, `${label}: accounts.json to store previewDelayMs ${delay}`)
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled, `${label}: the Appearance save bar to settle`)
  record.librariesKnownAfter = (await ctx.api("/api/libraries")).body?.known ?? null
  record.stored = storedAccount(ctx, userId)?.viewing?.startupDestination
  record.api = (await ctx.api("/api/settings/viewing")).body?.startupDestination
  ctx.writeEvidence(`accounts-after-${label}-delay.json`, ctx.readConfig("accounts.json"))
  return record
}

const SHOW_HOME = `(() => {
  ${FIND_QUERY}
  const viewing = query("viewing")
  viewing.setData({ ...viewing.state.data, startupDestination: "home" }, { manual: true })
  return viewing.state.data.startupDestination
})()`

const LEGACY_PATCH = `(async () => {
  const current = await (await fetch("/api/settings/viewing")).json()
  const response = await fetch("/api/settings/viewing", {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ ...current, startupDestination: "movies" }),
  })
  return { status: response.status, body: await response.text() }
})()`

async function saveTextSize(ctx, userId, label) {
  const field = { role: "spinbutton", name: "Text size percent" }
  const scale = storedAccount(ctx, userId)?.viewing?.textScale === 110 ? 120 : 110
  const record = { pageViewing: (await ctx.app.evaluate(PAGE_VIEWING))?.viewing, shown: (await ctx.state(SELECT)).value, textScale: scale }
  await ctx.fill(field, String(scale))
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => storedAccount(ctx, userId)?.viewing?.textScale === scale, `${label}: accounts.json to store textScale ${scale}`)
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled, `${label}: the Viewing save bar to settle`)
  record.stored = storedAccount(ctx, userId)?.viewing?.startupDestination
  ctx.writeEvidence(`accounts-after-${label}.json`, ctx.readConfig("accounts.json"))
  return record
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

    const resolved = (await ctx.api("/api/settings/viewing")).body?.startupDestination
    const refreshed = await ctx.until(async () => {
      const page = await ctx.app.evaluate(PAGE_VIEWING)
      return page && same(page.viewing, resolved) ? page : null
    }, "first launch: the page's cached viewing to match /api/settings/viewing", 20000).catch(() => null)
    await new Promise((settle) => setTimeout(settle, 2000))
    const after = await ctx.app.evaluate(PAGE_VIEWING)
    report.librariesChanged = { resolved, pageAfter: after, matchedAt: refreshed }
    expect(refreshed !== null, `first launch: once the library list is known, the page's cached viewing ${JSON.stringify(after?.viewing)} equals /api/settings/viewing ${JSON.stringify(resolved)}`)
    expect(after?.route === first.landing, `first launch: the page stayed on ${first.landing} once the library list was known (now ${after?.route})`)

    report.firstDelaySave = await savePreviewDelay(ctx, userId, "first")
    expect(same(report.firstDelaySave.stored, seeded), `first launch: saving only Card preview delay (list known: ${report.firstDelaySave.librariesKnownBefore} before, ${report.firstDelaySave.librariesKnownAfter} after; page viewing ${JSON.stringify(report.firstDelaySave.pageViewing)}) keeps ${JSON.stringify(seeded)} in accounts.json (now ${JSON.stringify(report.firstDelaySave.stored)})`)
    expect(same(report.firstDelaySave.api, resolved), `first launch: /api/settings/viewing still answers ${JSON.stringify(resolved)} after that save (now ${JSON.stringify(report.firstDelaySave.api)})`)

    await openThroughSidebar(ctx, report.expected.landing)
    await ctx.until(() => storedAccount(ctx, userId)?.browsing?.last === report.expected.landing, "the expected page saved as the last route", 10000)
    report.hashes.beforeRelaunch = accountsHash(ctx)
    ctx.writeEvidence("accounts-before-relaunch.json", ctx.readConfig("accounts.json"))
    await ctx.relaunch()
    const second = await land(ctx, "relaunch", userId)
    report.launches.push(second)
    report.hashes.afterRelaunch = accountsHash(ctx)
    ctx.writeEvidence("accounts-after-relaunch.json", ctx.readConfig("accounts.json"))
    const expectedView = expected ? { library: expected.id } : "home"
    expect(same(second.viewingAfterLanding, expectedView), `relaunch: after landing, the page's cached viewing is ${JSON.stringify(second.viewingAfterLanding)}, the destination resolved against the known library list`)
    expect(second.landing === report.expected.landing, `relaunch: ${JSON.stringify(seeded)} landed on ${second.landing} (expected ${report.expected.landing}${expected ? `, ${expected.name}` : ", Home"})`)
    expect(same(second.viewingApi, expected ? { library: expected.id } : "home"), `relaunch: /api/settings/viewing answers ${JSON.stringify(second.viewingApi)}`)
    expect(second.viewingUpdatesAfterLanding === 1 && second.viewingUpdatesSettled === 1, `relaunch: with the library list unchanged, the page holds the primed viewing without a refetch (updates ${second.viewingUpdatesAfterLanding} after landing, ${second.viewingUpdatesSettled} two seconds later)`)
    expect(report.hashes.afterRelaunch === report.hashes.beforeRelaunch, `relaunch: accounts.json kept SHA-256 ${report.hashes.beforeRelaunch}`)
    expect(same(storedAccount(ctx, userId)?.viewing?.startupDestination, seeded), `relaunch: accounts.json still stores ${JSON.stringify(seeded)}`)

    report.relaunchDelaySave = await savePreviewDelay(ctx, userId, "relaunch")
    expect(same(report.relaunchDelaySave.stored, seeded), `relaunch: saving only Card preview delay keeps ${JSON.stringify(seeded)} in accounts.json (now ${JSON.stringify(report.relaunchDelaySave.stored)})`)

    await openSettings(ctx, "Viewing")
    const shown = (await ctx.state(SELECT, { timeout: 30000 })).value
    report.settings = { shown, options: await startupOptions(ctx) }
    expect(shown === (expected ? expected.name : "Home"), `Settings shows the resolved destination "${shown}"`)
    expect(same(report.settings.options, ["Home", ...libraries.map((library) => library.name), "Releases", "Last browsing page"]), `the options are Home, ${libraries.length} libraries in server order, Releases and Last browsing page (${report.settings.options.join(", ")})`)

    report.textSizeSave = await saveTextSize(ctx, userId, "text-size")
    expect(same(report.textSizeSave.stored, seeded), `Viewing: saving only Text size while it shows "${report.textSizeSave.shown}" keeps ${JSON.stringify(seeded)} in accounts.json (now ${JSON.stringify(report.textSizeSave.stored)})`)
    report.staleTextSizeSave = { pageViewingSet: await ctx.app.evaluate(SHOW_HOME) }
    await ctx.until(async () => (await ctx.state(SELECT)).value === "Home", "Viewing: the select to show Home from the page's cached viewing")
    Object.assign(report.staleTextSizeSave, await saveTextSize(ctx, userId, "stale-text-size"))
    expect(same(report.staleTextSizeSave.stored, seeded), `Viewing: with the page's cached viewing set to Home, saving only Text size keeps ${JSON.stringify(seeded)} in accounts.json (now ${JSON.stringify(report.staleTextSizeSave.stored)})`)
    const restored = await ctx.until(async () => (await ctx.state(SELECT)).value === shown, `Viewing: the select to show "${shown}" from the save's answer`).catch(() => false)
    expect(restored, `Viewing: the select shows "${shown}" again from that save's answer (now "${(await ctx.state(SELECT)).value}")`)

    const beforeLegacy = accountsHash(ctx)
    report.legacyPatch = await ctx.app.evaluate(LEGACY_PATCH)
    expect(report.legacyPatch.status === 400, `a PATCH from the page naming "movies" is rejected with 400 (got ${report.legacyPatch.status}: ${report.legacyPatch.body.slice(0, 160)})`)
    expect(accountsHash(ctx) === beforeLegacy, "accounts.json is byte-identical after the rejected PATCH")

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
