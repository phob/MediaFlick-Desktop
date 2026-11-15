// The supported Jellyfin versions, against real servers: Desktop signs in to
// Jellyfin 10.11.0 and newer, and the Companion needs Jellyfin 12.0.0.
//
//   python3 .claude/skills/verify-mediaflick/scripts/jellyfin-version-servers.py 10.10.7 10.11.0 10.11.11 &
//   just verify server-versions --timeout 900 --log-level "debug,jellyfin.playstate=trace,playback=trace"
//
// The servers come from build/e2e/jellyfin-version-servers/servers.json, which
// that script writes (MEDIAFLICK_VERIFY_VERSION_SERVERS names another file).
// The demo server (12.x) runs last, read-only.
//
// - Older than 10.11.0: typing the address shows the refusal with both
//   versions, Sign in and Use Quick Connect stay disabled, the app's own
//   login and Quick Connect routes refuse with the same message, and the
//   server never sees a MediaFlick Desktop device (no credentials were sent).
// - 10.11.0 and newer, below 12: sign-in, Home, the Movies and Shows
//   libraries, a movie and a series detail page, Settings → MediaFlick
//   Companion saying it needs Jellyfin 12.0.0, then the movie plays for a few
//   seconds and stops, and sign-out.
// - The demo server: sign-in, the Companion page keeps its plain "not found".
//
// Seeded with the 0.1.6 profile the script writes (a config.json naming the
// first server, older than 10.11.0), the sign-in screen must show the
// refusal before anything is typed: the upgrade scenario "a server below the
// floor gets an explanation on the sign-in screen".
//   just verify server-versions --seed build/e2e/jellyfin-version-servers/upgrade-0-1-6 …
//
// Writes server-versions.json and a screenshot per state.
import { existsSync, readFileSync } from "node:fs"
import path from "node:path"

const DEMO = { url: "https://demo.jellyfin.org/stable", username: "demo", password: "" }
const DESKTOP_MINIMUM = [10, 11, 0]
const COMPANION_MINIMUM = "12.0.0"
const SERVER = { role: "textbox", name: "Server" }
const SIGN_IN = { role: "button", name: "Sign in" }
const QUICK_CONNECT = { role: "button", name: "Use Quick Connect" }
const CARD = (name) => ({ role: "link", name: `Open details for ${name}` })
const SIDEBAR = { css: '[data-sidebar="sidebar"]' }
const SETTINGS = { role: "link", name: "Settings" }
const WHATS_NEW = { role: "dialog", name: "What's new since MediaFlick 0.1.6" }
const numbers = (version) => version.split(".").map(Number)
const older = (left, right) => {
  for (let index = 0; index < 3; index++) if (left[index] !== right[index]) return left[index] < right[index]
  return false
}

function servers() {
  const file = process.env.MEDIAFLICK_VERIFY_VERSION_SERVERS ?? path.resolve(import.meta.dirname, "../../../../build/e2e/jellyfin-version-servers/servers.json")
  if (!existsSync(file)) throw new Error(`${file} is missing; start scripts/jellyfin-version-servers.py first`)
  return JSON.parse(readFileSync(file, "utf8"))
}

async function jellyfinAdmin(server) {
  const client = 'MediaBrowser Client="MediaFlick E2E", Device="e2e", DeviceId="mediaflick-version-servers", Version="1.0.0"'
  const auth = await fetch(`${server.url}/Users/AuthenticateByName`, {
    method: "POST",
    headers: { Authorization: client, "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify({ Username: server.username, Pw: server.password }),
  }).then((reply) => reply.json())
  const headers = { Authorization: `${client}, Token="${auth.AccessToken}"`, Accept: "application/json" }
  return {
    get: (route) => fetch(`${server.url}${route}`, { headers }).then((reply) => reply.json()),
    close: () => fetch(`${server.url}/Sessions/Logout`, { method: "POST", headers }),
  }
}

// Each server names its titles from whatever metadata it found online, so
// the drive asks it for the movie and the series the script added.
async function titles(server) {
  const admin = await jellyfinAdmin(server)
  const first = async (type) => (await admin.get(`/Items?Recursive=true&IncludeItemTypes=${type}&SortBy=SortName`)).Items[0].Name
  const names = { movie: await first("Movie"), series: await first("Series") }
  await admin.close()
  return names
}

async function desktopDevices(server) {
  const admin = await jellyfinAdmin(server)
  const { Items } = await admin.get("/Devices")
  await admin.close()
  return Items.filter((device) => device.AppName === "MediaFlick Desktop").map((device) => device.Name)
}

// The app's own routes, posted from the page as the UI would. A backstop for
// a sign-in sent before the address probe answered; the screen never sends it.
const postFromPage = (ctx, route, body) => ctx.app.evaluate(`fetch(${JSON.stringify(route)}, { method: "POST", headers: { "Content-Type": "application/json" }, body: ${JSON.stringify(JSON.stringify(body))} }).then(async (reply) => ({ status: reply.status, body: await reply.json() }))`)

async function refusal(ctx) {
  return ctx.until(() => ctx.app.evaluate(`document.querySelector('[role="alert"]')?.textContent.trim() ?? null`), "the refusal on the sign-in screen", 30000)
}

async function refused(ctx, server, report, { typed }) {
  const expected = `This server runs Jellyfin ${server.version}. MediaFlick needs Jellyfin ${DESKTOP_MINIMUM.join(".")} or newer.`
  if (typed) await ctx.fill(SERVER, server.url)
  const shown = await refusal(ctx)
  ctx.check(shown === expected, `the sign-in screen says "${shown}"`)
  await ctx.fill({ role: "textbox", name: "Username" }, server.username)
  await ctx.fill({ role: "textbox", name: "Password" }, server.password)
  ctx.check((await ctx.state(SIGN_IN)).disabled === true, "Sign in is disabled")
  ctx.check((await ctx.state(QUICK_CONNECT)).disabled === true, "Use Quick Connect is disabled")
  const help = await ctx.app.evaluate(`document.getElementById("quick-connect-help")?.textContent.trim() ?? ""`)
  await ctx.screenshot(`${server.version}-${typed ? "refused" : "refused-on-start"}`)
  await ctx.snapshot(`${server.version}-${typed ? "refused" : "refused-on-start"}`)
  const login = await postFromPage(ctx, "/api/auth/login", { server: server.url, username: server.username, password: server.password })
  ctx.check(login.status === 409 && login.body.error === expected, `the app's login route refuses with 409 and the same message (${login.status} ${login.body.error})`)
  const quick = await postFromPage(ctx, "/api/auth/quickconnect/start", { server: server.url })
  ctx.check(quick.status === 409 && quick.body.error === expected, `the app's Quick Connect route refuses with 409 (${quick.status} ${quick.body.error})`)
  const status = (await ctx.api("/api/status")).body
  ctx.check(status.authenticated === false, "the app stays signed out")
  const devices = await desktopDevices(server)
  ctx.check(devices.length === 0, `Jellyfin ${server.version} never saw a MediaFlick Desktop sign-in (devices: ${JSON.stringify(devices)})`)
  report.push({ version: server.version, url: server.url, typed, outcome: "refused", shown, quickConnectHelp: help, login, quickConnectStart: quick, desktopDevices: devices })
}

async function sidebarLink(ctx, name) {
  await ctx.hover(SIDEBAR)
  await ctx.press({ role: "link", name })
  await ctx.park()
}

async function stopFromBar(ctx) {
  const stop = { role: "button", name: "Stop" }
  const more = { role: "button", name: "More playback controls" }
  await ctx.revealPlayer()
  const direct = await ctx.until(async () => ((await ctx.exists(stop)) ? "direct" : (await ctx.exists(more)) ? "menu" : null), "the player bar's stop control", 15000)
  if (direct === "direct") return ctx.press(stop)
  await ctx.press(more)
  await ctx.press({ role: "menuitem", name: "Stop playback" })
}

async function companionPage(ctx) {
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "MediaFlick Companion" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/integrations/companion", "the Companion page")
  return ctx.until(() => ctx.app.evaluate(`(() => { const text = document.querySelector("main")?.innerText ?? ""; return /MediaFlick Companion (needs|was not found|is installed)[^\\n]*/.exec(text)?.[0] ?? null })()`), "the Companion connection text", 30000)
}

async function signOut(ctx, server) {
  // Signed out on a Settings page, the app stays in Settings; from Home it
  // returns to the sign-in screen.
  await ctx.hover(SIDEBAR)
  await ctx.press({ role: "link", name: "Home", within: SIDEBAR })
  await ctx.park()
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  const snapshot = await ctx.snapshot(`${server.version}-before-sign-out`)
  const menu = snapshot.match(new RegExp(`button '(\\S+ ${server.username} [^']*)'`))?.[1]
  if (!menu) throw new Error(`no user menu for ${server.username}`)
  await ctx.press({ role: "button", name: menu })
  await ctx.press({ role: "menuitem", name: "Sign out" })
  await ctx.find(SERVER, { timeout: 30000 })
  ctx.check((await ctx.api("/api/status")).body.authenticated === false, `signed out of ${server.version}`)
}

// The sign-in form, as ctx.signIn fills it; a 0.1.6 profile then opens its
// one-time What's new over Home, which hides the sidebar until it is closed.
async function signIn(ctx, server) {
  await ctx.fill({ role: "textbox", name: "Username" }, server.username)
  // A refused server's password may still be in the field; the demo's is empty.
  await ctx.fill({ role: "textbox", name: "Password" }, server.password)
  if (!server.password) await ctx.key("Backspace", { keyCode: 8 })
  await ctx.press(SIGN_IN)
  const shown = await ctx.until(async () => ((await ctx.exists(WHATS_NEW)) ? "dialog" : (await ctx.exists(SETTINGS)) ? "home" : null), "Home or What's new", 60000)
  if (shown === "dialog") {
    await ctx.press({ role: "button", name: "Got it", within: WHATS_NEW })
    await ctx.find(SETTINGS, { timeout: 30000 })
  }
}

async function supported(ctx, server, report) {
  const entry = { version: server.version, url: server.url, outcome: "signed in" }
  await ctx.fill(SERVER, server.url)
  await ctx.until(() => ctx.app.evaluate(`document.body.innerText.includes(${JSON.stringify(` · ${server.version}`)})`), `the probe naming ${server.version}`, 30000)
  ctx.check(!(await ctx.exists({ role: "alert" })), "no refusal for a supported server")
  await signIn(ctx, server)
  const status = (await ctx.api("/api/status")).body
  ctx.check(status.authenticated === true && status.userName === server.username, `/api/status reports ${server.username}`)
  await ctx.until(async () => {
    const now = (await ctx.api("/api/status")).body
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "the first sync to finish", 120000)
  await ctx.screenshot(`${server.version}-home`)

  const { movie, series } = server.username === DEMO.username ? {} : await titles(server)
  if (movie) {
    await sidebarLink(ctx, "Movies")
    await ctx.press(CARD(movie), { timeout: 30000 })
    await ctx.find({ role: "heading", name: movie }, { timeout: 30000 })
    await ctx.find({ role: "button", name: "Play" }, { timeout: 30000 })
    await ctx.screenshot(`${server.version}-movie`)
    const movieRoute = await ctx.pathname()
    await sidebarLink(ctx, "Shows")
    await ctx.press(CARD(series), { timeout: 30000 })
    await ctx.find({ role: "heading", name: series }, { timeout: 30000 })
    await ctx.find({ role: "list", name: "Seasons" }, { timeout: 30000 })
    await ctx.screenshot(`${server.version}-series`)
    entry.browsed = { movie, movieRoute, series, seriesRoute: await ctx.pathname() }
  }

  entry.companion = await companionPage(ctx)
  const tooOld = older(numbers(server.version), numbers(COMPANION_MINIMUM))
  const expected = tooOld
    ? `MediaFlick Companion needs Jellyfin ${COMPANION_MINIMUM} or newer. This server runs Jellyfin ${server.version}.`
    : "MediaFlick Companion was not found on this Jellyfin server."
  ctx.check(entry.companion === expected, `the Companion page says "${entry.companion}"`)
  entry.companionStatus = (await ctx.api("/api/companion/info")).body
  await ctx.screenshot(`${server.version}-companion`)

  if (movie) {
    await sidebarLink(ctx, "Movies")
    await ctx.press(CARD(movie), { timeout: 30000 })
    await ctx.press({ role: "button", name: "Play" })
    const playing = await ctx.until(async () => {
      const state = await ctx.player()
      return state?.active && state.positionMs > 3000 ? state : null
    }, "the movie playing past 3 s", 90000)
    if (process.platform === "linux") ctx.displayCapture(`${server.version}-playing`)
    await stopFromBar(ctx)
    await ctx.until(async () => !(await ctx.player())?.active, "playback to stop", 30000)
    await ctx.until(() => ctx.playstates().sent.some((sent) => sent.endpoint === "Sessions/Playing/Stopped" && sent.itemId === playing.itemId && sent.status === 204), `Jellyfin ${server.version} to accept the Stopped report (204)`, 30000)
    entry.played = { itemId: playing.itemId, playMethod: playing.playMethod, positionMs: playing.positionMs }
    entry.desktopDevices = await desktopDevices(server)
    ctx.check(entry.desktopDevices.length > 0, `Jellyfin ${server.version} lists the app's device after sign-in`)
  }

  await signOut(ctx, server)
  report.push(entry)
}

export default async function serverVersions(ctx) {
  const all = servers()
  const report = []
  await ctx.find(SERVER, { timeout: 30000 })
  const prefilled = (await ctx.find(SERVER)).properties.value
  const seeded = all.find((server) => server.url === prefilled)
  if (seeded) {
    ctx.check(older(numbers(seeded.version), DESKTOP_MINIMUM), `the seeded server ${seeded.version} is below the floor`)
    await refused(ctx, seeded, report, { typed: false })
  }
  for (const server of all) {
    if (older(numbers(server.version), DESKTOP_MINIMUM)) await refused(ctx, server, report, { typed: true })
    else await supported(ctx, server, report)
  }
  const demo = await fetch(`${DEMO.url}/System/Info/Public`).then((reply) => reply.json())
  await supported(ctx, { ...DEMO, version: demo.Version }, report)
  ctx.writeEvidence("server-versions.json", JSON.stringify({ servers: report }, null, 2))
}
