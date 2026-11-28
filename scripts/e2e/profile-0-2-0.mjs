import { readdirSync, readFileSync, statSync } from "node:fs"
import path from "node:path"

const POSTER = process.env.FIXTURE_POSTER ?? new URL("./profile-0-2-0-poster.png", import.meta.url).pathname
const LETTERBOXD_USER = "davidehrlich"
const SIDEBAR = { css: '[data-sidebar="sidebar"]' }
const TARGET = { css: "[data-verify-target]" }
const COLLECTION_SETTINGS = { role: "link", name: "Collections", within: { role: "navigation", name: "Settings navigation" } }
const today = () => new Date().toLocaleDateString("sv-SE")
const account = (ctx) => ctx.readConfig("accounts.json")?.accounts?.find((entry) => entry.userId === ctx.account.userId) ?? null
const profiles = (ctx) => ctx.readConfig("collections.json")?.accounts?.flatMap((entry) => entry.profiles ?? []) ?? []
const mark = (ctx, selector, text) => ctx.app.evaluate(`(() => {
  document.querySelectorAll("[data-verify-target]").forEach((node) => node.removeAttribute("data-verify-target"))
  const node = [...document.querySelectorAll(${JSON.stringify(selector)})].find((candidate) => candidate.textContent.includes(${JSON.stringify(text)}))
  node?.setAttribute("data-verify-target", "")
  return node ? node.textContent.replace(/\\s+/g, " ").trim() : null
})()`)

async function sidebarLink(ctx, name) {
  await ctx.hover(SIDEBAR)
  await ctx.press({ role: "link", name })
}
async function openShelf(ctx, name) {
  if (!(await ctx.exists({ role: "navigation", name: "Settings navigation" }))) await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name, within: { role: "navigation", name: "Settings navigation" } })
}
async function dismissUpdateToast(ctx) {
  const dismiss = { role: "button", name: "Dismiss update notification" }
  if (await ctx.exists(dismiss)) await ctx.press(dismiss)
  else await ctx.app.evaluate(`document.getElementById("__mediaFlickDesktopUpdateToast")?.remove()`)
}
async function saveShelf(ctx) {
  await dismissUpdateToast(ctx)
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "the shelf to be clean after Save", 60000)
}
function configFiles(ctx) {
  const dir = path.dirname(ctx.configFile("settings.json"))
  const walk = (base, prefix = "") => readdirSync(base, { withFileTypes: true }).flatMap((entry) => {
    const relative = path.posix.join(prefix, entry.name)
    return entry.isDirectory() ? walk(path.join(base, entry.name), relative) : [{ name: relative, size: statSync(path.join(base, entry.name)).size }]
  })
  return { dir, files: walk(dir) }
}

export default async function makeFixture(ctx) {
  const intent = {}
  await ctx.signIn({})
  await ctx.until(async () => {
    const now = (await ctx.api("/api/status")).body
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "the first sync to finish", 600000)
  intent.status = (await ctx.api("/api/status")).body
  await ctx.screenshot("01-home-synced")

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "the Player shelf")
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  await ctx.choose({ name: "Default fullscreen" }, "Windowed")
  await saveShelf(ctx)
  await ctx.press({ role: "link", name: "Application" })
  await ctx.find({ role: "switch", name: "Show scrollbars" })
  await ctx.press({ role: "switch", name: "Show scrollbars" })
  await saveShelf(ctx)
  intent.settings = ctx.readConfig("settings.json")
  ctx.check(intent.settings?.default_fullscreen === "windowed" && intent.settings?.show_scrollbars === true, "settings.json holds Windowed and Show scrollbars")

  await openShelf(ctx, "Viewing")
  await ctx.find({ role: "switch", name: "Hide watched by default" }, { timeout: 20000 })
  await ctx.press({ role: "switch", name: "Hide watched by default" })
  await ctx.fill({ role: "textbox", name: "Audio languages" }, "en, de")
  await saveShelf(ctx)
  await openShelf(ctx, "Appearance")
  await ctx.choose({ name: "Accent" }, "Cobalt")
  await ctx.press({ role: "checkbox", name: "IMDb" })
  await ctx.press({ role: "checkbox", name: "Rotten Tomatoes Critics" })
  await saveShelf(ctx)
  await openShelf(ctx, "Letterboxd")
  await ctx.fill({ role: "textbox", name: "Letterboxd username or profile URL" }, LETTERBOXD_USER)
  await ctx.press({ role: "button", name: "Add profile" })
  await dismissUpdateToast(ctx)
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => account(ctx)?.letterboxd_profiles?.length > 0, "accounts.json to hold the Letterboxd profile", 60000)
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "the Letterboxd shelf to be clean", 60000)
  intent.account = account(ctx)
  ctx.check(intent.account?.viewing?.hideWatched === true && intent.account?.appearance?.accent === "cobalt", "accounts.json holds Viewing and Appearance")
  await ctx.screenshot("02-letterboxd-saved")

  await sidebarLink(ctx, "Movie Franchises")
  const franchises = await ctx.until(async () => {
    const body = (await ctx.api(`/api/collections/franchises?localDate=${today()}`)).body
    return body?.status === "ready" ? body : null
  }, "the franchise snapshot to be ready", 300000)
  intent.franchises = franchises.franchises?.length ?? null
  await ctx.park()
  await ctx.screenshot("03-franchises")

  await sidebarLink(ctx, "My Collections")
  await ctx.find({ role: "heading", name: "No collections yet" }, { timeout: 30000 })
  await ctx.press({ role: "link", name: "Choose templates" })
  await ctx.find({ role: "textbox", name: "Search templates" }, { timeout: 30000 })
  intent.template = await ctx.until(() => mark(ctx, "main button", "TMDB"), "a TMDB template", 20000)
  await ctx.press(TARGET)
  await ctx.find({ role: "dialog", name: "Add collection" }, { timeout: 15000 })
  const title = await ctx.app.evaluate(`document.getElementById("collection-title")?.value ?? null`)
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.app.evaluate(`/\\d+ total ·/.test(document.querySelector('[role="dialog"]')?.textContent ?? "")`), "the preview before saving", 120000)
  const { root } = await ctx.app.send("DOM.getDocument", { depth: -1, pierce: true })
  const { nodeId } = await ctx.app.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[aria-label="Custom poster file"]' })
  if (!nodeId) throw new Error("no Custom poster file input in the dialog")
  await ctx.app.send("DOM.setFileInputFiles", { nodeId, files: [POSTER] })
  await ctx.until(() => ctx.app.evaluate(`document.getElementById("collection-poster-status")?.textContent.trim() ?? null`).then((text) => text && !/Uploading/.test(text) ? text : null), "the poster upload to finish", 60000)
  intent.posterStatus = await ctx.app.evaluate(`document.getElementById("collection-poster-status")?.textContent.trim()`)
  await ctx.screenshot("04-collection-with-poster")
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => profiles(ctx).some((profile) => profile.title === title), `collections.json to hold ${title}`, 240000)
  intent.collection = profiles(ctx).find((profile) => profile.title === title)
  ctx.check(Boolean(intent.collection?.customPosterId), "the collection keeps its custom poster id")
  await sidebarLink(ctx, "My Collections")
  await ctx.press({ role: "link", name: `Open ${title}` })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/collections/mine/"), "the collection's page")
  await ctx.until(() => ctx.app.evaluate(`document.querySelectorAll("main a[aria-label]").length > 0`), "the collection's titles", 120000)
  await ctx.screenshot("05-my-collection")

  const movies = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=Movie&fields=MediaStreams,ProviderIds&sortBy=SortName&limit=300`)
  const streams = (item, type) => (item.MediaStreams ?? []).filter((stream) => stream.Type === type).length
  const movie = movies.Items.find((item) => streams(item, "Subtitle") > 0 && item.ProviderIds?.Imdb) ?? movies.Items.find((item) => streams(item, "Audio") > 1)
  if (!movie) throw new Error("no movie with a subtitle or second audio track")
  intent.movie = { id: movie.Id, name: movie.Name, subtitles: streams(movie, "Subtitle"), audio: streams(movie, "Audio") }
  await sidebarLink(ctx, "Movies")
  await ctx.until(() => ctx.app.evaluate(`document.querySelectorAll('dl[aria-label^="Ratings for "]').length > 0`), "rating readouts on movie cards", 90000)
  await ctx.app.route(`/item/${movie.Id}`)
  await ctx.find({ role: "heading", name: movie.Name }, { timeout: 30000 })
  const selectName = intent.movie.subtitles > 0 ? "Subtitle track" : "Audio track"
  await ctx.app.evaluate(`document.getElementById(${JSON.stringify(selectName === "Subtitle track" ? "subtitle-track-select" : "audio-track-select")})?.scrollIntoView({ block: "center" })`)
  const current = (await ctx.state({ role: "combobox", name: selectName })).value
  await ctx.press({ role: "combobox", name: selectName })
  const options = await ctx.until(() => ctx.app.evaluate(`[...document.querySelectorAll('[role="option"]')].map((node) => node.textContent.trim())`).then((names) => names.length ? names : null), `the ${selectName} options`)
  const choice = options.find((name) => name !== current && name !== "Subtitles off") ?? options.find((name) => name !== current)
  await ctx.press({ role: "option", name: choice })
  intent.playbackPreference = { select: selectName, from: current, to: choice }
  await ctx.until(() => JSON.stringify(ctx.readConfig("playback-preferences.json") ?? {}).includes(movie.Id), "playback-preferences.json to hold the choice", 30000)
  intent.playbackPreferences = ctx.readConfig("playback-preferences.json")
  await ctx.screenshot("06-playback-preference")

  intent.ratingSettings = (await ctx.api("/api/settings")).body?.appearance?.ratingSources ?? null
  intent.config = configFiles(ctx)
  await ctx.writeEvidence("fixture-intent.json", JSON.stringify(intent, null, 2))
  ctx.step(`fixture intent recorded: collection ${title}, movie ${movie.Name}`)
}
