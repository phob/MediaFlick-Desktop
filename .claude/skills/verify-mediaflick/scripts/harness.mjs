// The `ctx` a drive receives. Elements are found the way assistive technology
// sees them (CDP Accessibility.queryAXTree: role + accessible name) and
// operated with real CDP mouse and keyboard input, so React sees the same
// events a user produces.

import { execFileSync } from "node:child_process"
import { appendFileSync, existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs"
import net from "node:net"
import path from "node:path"
import { sleep } from "../../../../scripts/website/cdp.mjs"
import { approveQuickConnect, cancelSeerrRequest, jellyfinGet, readStoredSession, resetUserData, revokeToken, saveAccount as cacheAccount, publicAccount, tokenStatus } from "./account.mjs"
import { isLinux, isWindows, listProcesses, sendWindowClose } from "./platform.mjs"
import { startProxy } from "./proxy.mjs"

const SKIPPED_ROLES = new Set(["InlineTextBox", "LineBreak", "none", "generic", "ignored"])
const STATE_PROPERTIES = ["focused", "disabled", "checked", "pressed", "expanded", "selected", "invalid", "required", "readonly"]

export function createHarness({ app: firstApp, currentChild, evidence, configDir, dataDir, log, relaunchApp, windowShown, account = null, cachedAccount = null, signOutBlocked = () => false, rememberSecret = () => {} }) {
  // Replaced by `relaunch`; every helper reads the current app.
  let app = firstApp
  const proxies = []
  const afterRunTasks = []
  const step = (text) => log(text)

  function check(condition, message) {
    if (!condition) throw new Error(`check failed: ${message}`)
    log(`ok: ${message}`)
  }

  // Polls a JS expression in the page, or an async function in Node, until truthy.
  async function until(condition, description, timeout = 20000, { quiet = false } = {}) {
    const deadline = Date.now() + timeout
    let last
    while (Date.now() < deadline) {
      if (signOutBlocked()) throw new Error("the drive tried to sign out of a cached Jellyfin account; Sign out revokes its token on the server, so --account and verify-login runs block it")
      try {
        last = typeof condition === "function" ? await condition() : await app.evaluate(condition)
        if (last) {
          if (!quiet) log(`saw ${description}`)
          return last
        }
      } catch (error) {
        last = error.message
      }
      await sleep(200)
    }
    throw new Error(`timed out after ${timeout} ms waiting for ${description} (last: ${JSON.stringify(last)})`)
  }

  // { role, name } goes through the accessibility tree; { css } is the
  // fallback for controls without a stable accessible name.
  async function axQuery({ role, name, css }) {
    const { root } = await app.send("DOM.getDocument", { depth: 0 })
    if (css) {
      const { nodeIds } = await app.send("DOM.querySelectorAll", { nodeId: root.nodeId, selector: css })
      const nodes = []
      for (const nodeId of nodeIds) nodes.push({ backendDOMNodeId: (await app.send("DOM.describeNode", { nodeId })).node.backendNodeId })
      return nodes
    }
    const { nodes } = await app.send("Accessibility.queryAXTree", { nodeId: root.nodeId, ...(role ? { role } : {}), ...(name !== undefined ? { accessibleName: name } : {}) })
    return nodes.filter((node) => !node.ignored && node.backendDOMNodeId)
  }

  async function box(backendNodeId) {
    try {
      const { model } = await app.send("DOM.getBoxModel", { backendNodeId })
      const [x1, y1, x2, , , y3] = model.border
      if (model.width === 0 || model.height === 0) return null
      return { x: (x1 + x2) / 2, y: (y1 + y3) / 2, width: model.width, height: model.height }
    } catch {
      return null
    }
  }

  const describe = ({ role, name, css }) => (css ? `css ${css}` : `${role ?? "*"} ${JSON.stringify(name ?? "*")}`)

  // Waits for exactly one rendered element with this role and accessible name.
  // `nth` picks among several on purpose; otherwise duplicates are an error.
  async function find(target, { timeout = 15000, nth } = {}) {
    return until(async () => {
      const matches = []
      for (const node of await axQuery(target)) {
        const rect = await box(node.backendDOMNodeId)
        if (rect) matches.push({ ...target, backendNodeId: node.backendDOMNodeId, properties: props(node), rect })
      }
      if (matches.length > 1 && nth === undefined) throw new Error(`${matches.length} elements match ${describe(target)}; pass { nth } or a more specific name`)
      return matches[nth ?? 0] ?? null
    }, `${describe(target)} to be on screen`, timeout, { quiet: true })
  }

  // How many rendered elements match (for lists, grids and shelves).
  async function count(target) {
    let total = 0
    for (const node of await axQuery(target)) if (await box(node.backendDOMNodeId)) total++
    return total
  }

  async function exists(target) {
    for (const node of await axQuery(target)) if (await box(node.backendDOMNodeId)) return true
    return false
  }

  function props(node) {
    const result = {}
    for (const property of node.properties ?? []) {
      if (!STATE_PROPERTIES.includes(property.name)) continue
      // checked and pressed are tristate tokens ("true", "false", "mixed").
      const value = property.value?.value
      result[property.name] = value === "true" ? true : value === "false" ? false : value
    }
    if (node.value?.value !== undefined) result.value = node.value.value
    return result
  }

  // Current state properties (checked, expanded, value, ...) of an element.
  async function state(target, options) {
    return (await find(target, options)).properties
  }

  // A click can close the app (`Discard and leave`). The app may then exit
  // before CEF answers the mouse event, and the CDP client never settles a
  // request on a closed socket, so each event also ends when the app exits.
  async function mouseAt(x, y) {
    const event = { x, y, button: "left", clickCount: 1 }
    const exited = currentChild().exited
    for (const params of [{ type: "mouseMoved", x, y, button: "none" }, { type: "mousePressed", ...event }, { type: "mouseReleased", ...event }]) {
      await Promise.race([app.send("Input.dispatchMouseEvent", params), exited])
    }
  }

  // What the pointer would hit at (x, y): the target itself, a descendant, or
  // the name of whatever covers it.
  async function hitTest(backendNodeId, x, y) {
    const { backendNodeId: hit } = await app.send("DOM.getNodeForLocation", { x: Math.round(x), y: Math.round(y) })
    const { object: target } = await app.send("DOM.resolveNode", { backendNodeId })
    const { object: covering } = await app.send("DOM.resolveNode", { backendNodeId: hit })
    const { result } = await app.send("Runtime.callFunctionOn", {
      objectId: target.objectId,
      // A Sonner toast is reported as such: it covers the bottom-right corner
      // (the settings Save button) for a few seconds, then leaves on its own.
      functionDeclaration: "function (hit) { if (this === hit || this.contains(hit)) return true; const toast = hit.closest('[data-sonner-toast]'); return toast ? `toast: ${toast.textContent.trim().slice(0, 80)}` : (hit.closest('a,button,[role]')?.outerHTML ?? hit.outerHTML).slice(0, 160) }",
      arguments: [{ objectId: covering.objectId }],
      returnByValue: true,
    })
    return result.value
  }

  // Moves the pointer to the quiet top-right corner, as a user moves it away
  // from the sidebar (which expands under the pointer and overlays content).
  async function park() {
    const [width] = await app.evaluate("[innerWidth, innerHeight]")
    await app.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: width - 4, y: 4, button: "none" })
    await sleep(400)
  }

  // Scrolling only into view leaves a target at the viewport's edge, where a
  // sticky bar (the settings save bar, once a draft exists) can cover it.
  async function centre(backendNodeId) {
    const { object } = await app.send("DOM.resolveNode", { backendNodeId })
    await app.send("Runtime.callFunctionOn", { objectId: object.objectId, functionDeclaration: "function () { this.scrollIntoView({ block: 'center', inline: 'nearest' }) }" })
  }

  // Scrolls the element into view and returns a point the pointer really hits.
  // The element must hold still first: leaving Home turns the pinned sidebar
  // into a collapsed overlay and slides the content left, and a click at the
  // old position lands on nothing. A toast covering it is waited out. Any
  // other covered target first gets time to settle (transitions end, the
  // expanding sidebar finishes), then is scrolled to the middle of the
  // viewport, out from under sticky bars; only then is the pointer parked, because
  // moving it away also closes hover-opened UI such as the sidebar. React may
  // re-mount the element meanwhile; a detached node is found again rather
  // than reported.
  async function reach(target, options) {
    let covered = 0
    let moving = 0
    let toasted = 0
    let previous = null
    for (let attempt = 0; attempt < 80; attempt++) {
      const element = await find(target, options)
      try {
        await app.send("DOM.scrollIntoViewIfNeeded", { backendNodeId: element.backendNodeId })
        const rect = (await box(element.backendNodeId)) ?? element.rect
        const still = previous && Math.abs(previous.x - rect.x) < 1 && Math.abs(previous.y - rect.y) < 1
        previous = rect
        if (!still) {
          if (++moving === 30) throw new Error(`${describe(target)} kept moving`)
          await sleep(100)
          continue
        }
        const hit = await hitTest(element.backendNodeId, rect.x, rect.y)
        if (hit === true) return { element, rect }
        if (typeof hit === "string" && hit.startsWith("toast: ")) {
          if (++toasted === 30) throw new Error(`${describe(target)} is covered by ${hit}`)
          // Sonner pauses a toast while the pointer rests on it, and the
          // pointer is often still where Save was pressed, under the toast.
          if (toasted === 1) await park()
          await sleep(300)
          continue
        }
        covered++
        if (covered === 12) throw new Error(`${describe(target)} is covered by ${hit}`)
        if (covered === 3) await centre(element.backendNodeId)
        else if (covered === 6) await park()
        else await sleep(150)
      } catch (error) {
        if (!/detached|No node|Could not find node/i.test(error.message)) throw error
        previous = null
        await sleep(200)
      }
    }
    throw new Error(`${describe(target)} kept detaching from the document`)
  }

  // Clicks the element's centre with the mouse once nothing covers it.
  async function press(target, options) {
    const { element, rect } = await reach(target, options)
    if (element.properties.disabled) throw new Error(`${describe(target)} is disabled`)
    log(`press ${describe(target)}`)
    await mouseAt(rect.x, rect.y)
    return element
  }

  // Starts from the parked corner so the move always enters the element
  // (pointerenter fires; the sidebar expands on it).
  async function hover(target, options) {
    await park()
    const { rect } = await reach(target, options)
    log(`hover ${describe(target)}`)
    await app.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: rect.x, y: rect.y, button: "none" })
  }

  async function key(name, { code = name, keyCode = 0, modifiers = 0 } = {}) {
    const event = { key: name, code, windowsVirtualKeyCode: keyCode, modifiers }
    await app.send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...event })
    await app.send("Input.dispatchKeyEvent", { type: "keyUp", ...event })
  }

  // Focuses a text field, selects its content and types over it.
  async function fill(target, text) {
    const { rect } = await reach(target)
    log(`fill ${describe(target)} with ${JSON.stringify(text)}`)
    await mouseAt(rect.x, rect.y)
    await key("a", { code: "KeyA", keyCode: 65, modifiers: process.platform === "darwin" ? 4 : 2 })
    if (text === "") await key("Backspace", { keyCode: 8 })
    else await app.send("Input.insertText", { text })
  }

  // Opens a Radix Select (role combobox, named by its label) and picks an option.
  async function choose(target, optionName) {
    await press({ role: "combobox", ...target })
    await press({ role: "option", name: optionName })
    await until(async () => (await state({ role: "combobox", ...target })).expanded !== true, "the listbox to close", 5000, { quiet: true })
  }

  // Lets transitions (switch thumbs, dialogs, toasts) finish before capture.
  async function screenshot(name, { settle = 300 } = {}) {
    await sleep(settle)
    const { data } = await app.send("Page.captureScreenshot", { format: "png" })
    const file = path.join(evidence, `${name}.png`)
    writeFileSync(file, Buffer.from(data, "base64"))
    log(`screenshot ${name}.png`)
    return file
  }

  // The accessibility tree as text: one line per node, `role 'name' state`.
  async function snapshot(name) {
    const { nodes } = await app.send("Accessibility.getFullAXTree")
    const byId = new Map(nodes.map((node) => [node.nodeId, node]))
    const lines = []
    const visit = (node, depth) => {
      const role = node.role?.value ?? "?"
      const label = node.name?.value ?? ""
      const shown = !node.ignored && !SKIPPED_ROLES.has(role) && !(role === "StaticText" && !label.trim())
      if (shown) {
        const flags = Object.entries(props(node)).filter(([, value]) => value !== false && value !== "").map(([key, value]) => (value === true ? key : `${key}=${JSON.stringify(value)}`))
        lines.push(`${"  ".repeat(depth)}${role}${label ? ` '${label.replace(/\s+/g, " ").slice(0, 120)}'` : ""}${flags.length ? ` [${flags.join(" ")}]` : ""}`)
      }
      for (const child of node.childIds ?? []) {
        const next = byId.get(child)
        if (next) visit(next, shown ? depth + 1 : depth)
      }
    }
    const top = nodes.find((node) => !node.parentId)
    if (top) visit(top, 0)
    const file = path.join(evidence, `${name}.ax.txt`)
    writeFileSync(file, `${lines.join("\n")}\n`)
    log(`snapshot ${name}.ax.txt`)
    return lines.join("\n")
  }

  // Reads app state through the same /api the UI uses. For verifying results
  // only: a proof must not perform its action through this.
  async function api(apiPath) {
    return app.evaluate(`fetch(${JSON.stringify(apiPath)}).then(async (response) => ({ status: response.status, body: await response.json().catch(() => null) }))`)
  }

  const pathname = () => app.evaluate("location.pathname + location.search")

  // Files the app wrote into the disposable profile.
  const configFile = (name) => path.join(configDir, name)
  const readConfig = (name) => (existsSync(configFile(name)) ? JSON.parse(readFileSync(configFile(name), "utf8")) : null)
  const configStat = (name) => (existsSync(configFile(name)) ? statSync(configFile(name)) : null)
  // The Rust log so far (copied to app.log at the end of the run).
  const appLog = () => (existsSync(configFile("mediaflick-desktop.log")) ? readFileSync(configFile("mediaflick-desktop.log"), "utf8") : "")

  function writeEvidence(name, value) {
    writeFileSync(path.join(evidence, name), typeof value === "string" ? value : `${JSON.stringify(value, null, 2)}\n`)
    log(`evidence ${name}`)
  }

  // The app's own playback snapshot: active, itemId, playMethod, positionMs,
  // durationMs, paused, tracks, skipSegments, stopReason, diagnostics.
  async function player() {
    return (await api("/api/player/state")).body
  }

  // The pipe to read mpv on. mpv's own scripts can move its IPC server after
  // the app has connected: SVP's mpvSockets.lua, shipped in many Windows mpv
  // bundles and loaded by External mpv (`--load-scripts=yes`), re-points
  // input-ipc-server to <TEMP>\mpvSockets\<pid>. The logged pipe's name
  // lingers, but a new client there never connects. Follow the move to the
  // pipe named after the mpv process launched with the logged one.
  function mpvSocketPath(logged) {
    if (!isWindows) return logged
    const prefix = "\\\\.\\pipe\\"
    const launched = listProcesses().find((entry) => entry.cmd.includes(`--input-ipc-server=${logged}`))
    const moved = launched && readdirSync(prefix).find((pipe) => pipe.endsWith(`\\mpvSockets\\${launched.pid}`))
    if (!moved) return logged
    log(`mpv IPC moved by a script: ${prefix}${moved}`)
    return `${prefix}${moved}`
  }

  // One connection per mpv IPC path, kept for the drive, so a read never
  // depends on mpv accepting a new client mid-playback.
  let mpvClient = null
  function mpvConnection(logged) {
    if (mpvClient?.logged === logged && !mpvClient.socket.destroyed) return mpvClient
    mpvClient?.socket.destroy()
    const pending = new Map()
    const socket = net.createConnection(mpvSocketPath(logged))
    const fail = (error) => {
      for (const { reject } of pending.values()) reject(error)
      pending.clear()
    }
    let buffer = ""
    socket.on("data", (chunk) => {
      buffer += chunk
      for (const line of buffer.split("\n").slice(0, -1)) {
        const message = JSON.parse(line)
        const waiting = pending.get(message.request_id)
        if (!waiting) continue // events and other replies
        pending.delete(message.request_id)
        waiting.resolve(message)
      }
      buffer = buffer.slice(buffer.lastIndexOf("\n") + 1)
    })
    socket.on("error", fail)
    socket.on("close", () => fail(new Error("the mpv IPC connection closed")))
    socket.unref()
    mpvClient = { logged, socket, pending }
    return mpvClient
  }

  // Reads one mpv property over mpv's IPC socket, beside the app's own
  // connection. Reads only: the app must stay mpv's single command writer.
  async function mpv(property) {
    const socketPath = [...appLog().matchAll(/mpv IPC connected ipc_path=(\S+)/g)].at(-1)?.[1]
    if (!socketPath) throw new Error("app.log has no `mpv IPC connected` line yet")
    const { socket, pending } = mpvConnection(socketPath)
    const requestId = Math.floor(Math.random() * 1e9)
    const message = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(requestId)
        // Drop the connection so the next read resolves the pipe again.
        socket.destroy()
        reject(new Error(`mpv did not answer get_property ${property} within 5 s`))
      }, 5000)
      pending.set(requestId, {
        resolve: (reply) => { clearTimeout(timer); resolve(reply) },
        reject: (error) => { clearTimeout(timer); reject(error) },
      })
      socket.write(`${JSON.stringify({ command: ["get_property", property], request_id: requestId })}\n`)
    })
    if (message.error === "success") return message.data
    if (message.error === "property unavailable") return null
    throw new Error(`mpv get_property ${property}: ${message.error}`)
  }

  // The playstate reports the app queued for Jellyfin and the answers it got,
  // from app.log. Needs `--log-level "debug,jellyfin.playstate=trace"`.
  function playstates() {
    const queued = []
    const sent = []
    for (const line of appLog().split("\n")) {
      const report = line.match(/^(\S+)\s+TRACE.*jellyfin\.playstate: queued Jellyfin playback state endpoint="([^"]+)" item_id=(\S+).*? state=position=(\d+)/)
      if (report) queued.push({ at: report[1], endpoint: report[2], itemId: report[3], positionS: Number(report[4]) / 1e7 })
      const answer = line.match(/^(\S+)\s+TRACE.*jellyfin\.playstate: sent Jellyfin playback state endpoint="([^"]+)" item_id=(\S+) status=(\d+)/)
      if (answer) sent.push({ at: answer[1], endpoint: answer[2], itemId: answer[3], status: Number(answer[4]) })
    }
    if (!queued.length && !/jellyfin\.playstate=trace/.test(appLog())) throw new Error("playstates() needs --log-level \"debug,jellyfin.playstate=trace\"")
    return { queued, sent }
  }

  // The integrated player bar hides after 3 s and turns `inert`; a real
  // pointer move near the bottom edge brings it back for ARIA lookups.
  async function revealPlayer() {
    const [width, height] = await app.evaluate("[innerWidth, innerHeight]")
    for (const [x, y] of [[width / 2 - 40, height - 60], [width / 2, height - 40]]) {
      await app.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "none" })
      await sleep(150)
    }
  }

  // Linux only: the whole X display as the user would see it, including the
  // mpv video that CDP screenshots (the CEF layer alone) do not contain. With
  // `seconds`, an MP4 clip instead of a PNG.
  function displayCapture(name, { seconds } = {}) {
    if (!isLinux) throw new Error("displayCapture uses ffmpeg x11grab and runs on Linux only")
    const file = path.join(evidence, seconds ? `${name}.mp4` : `${name}.png`)
    const input = ["-hide_banner", "-loglevel", "error", "-f", "x11grab", "-i", process.env.DISPLAY]
    const output = seconds ? ["-t", String(seconds), "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p"] : ["-frames:v", "1"]
    execFileSync("ffmpeg", [...input, ...output, "-y", file], { stdio: "ignore", timeout: ((seconds ?? 0) + 20) * 1000 })
    log(`display capture ${path.basename(file)}`)
    return file
  }

  // Signs in through the sign-in form. With no server given, the field must
  // already hold one (pass `--url` to the session). Under `--account` the app
  // starts signed in, so this only confirms the cached account and ignores
  // the arguments.
  async function signIn({ server, username, password = "" }) {
    if (account?.signedOut) {
      await quickConnect({ approve: true, timeout: 60000 })
      return
    }
    if (account && !account.login) {
      await find({ role: "link", name: "Settings" }, { timeout: 60000 })
      const status = (await api("/api/status")).body
      if (status?.authenticated !== true || status.userName !== account.userName) throw new Error(`the app did not start signed in as ${account.userName} from the cached session (status: ${JSON.stringify({ authenticated: status?.authenticated, expired: status?.expired, userName: status?.userName })})`)
      log(`signed in from the cached session as ${account.userName}`)
      return
    }
    await find({ role: "button", name: "Sign in" }, { timeout: 30000 })
    if (server) await fill({ role: "textbox", name: "Server" }, server)
    await fill({ role: "textbox", name: "Username" }, username)
    if (password) await fill({ role: "textbox", name: "Password" }, password)
    await press({ role: "button", name: "Sign in" })
    await find({ role: "link", name: "Settings" }, { timeout: 60000 })
    log(`signed in as ${username}`)
  }

  // Signs in through the sign-in screen's Quick Connect and waits for the
  // user to approve the code in Jellyfin. The server comes from the Server
  // field (`--url`, or the server `just verify-login` was given).
  // With `approve` (only under `--account … --signed-out`) the harness approves
  // the code itself as the cached account.
  async function quickConnect({ timeout = 600000, approve = false } = {}) {
    const server = await find({ role: "textbox", name: "Server" }, { timeout: 30000 })
    const useQuickConnect = { role: "button", name: "Use Quick Connect" }
    const help = () => app.evaluate(`document.getElementById("quick-connect-help")?.textContent.trim() ?? ""`)
    await until(async () => {
      const text = await help()
      if (/not enabled|Could not check/.test(text)) throw new Error(`Quick Connect is unavailable on ${server.properties.value}: ${text}`)
      return !(await state(useQuickConnect)).disabled
    }, "Use Quick Connect enabled", 30000)
    await press(useQuickConnect)
    const code = await until(async () => (await app.evaluate("document.body.innerText")).includes("Waiting for approval") && app.evaluate(`document.querySelector('[aria-label="Copy code"]')?.previousElementSibling?.textContent ?? null`), "a Quick Connect code", 20000)
    await screenshot("quick-connect-code")
    if (approve) {
      if (!cachedAccount || !account?.signedOut) throw new Error("quickConnect({ approve: true }) needs --account SERVER --signed-out")
      await approveQuickConnect(cachedAccount, code)
      log(`approved Quick Connect code ${code} as ${cachedAccount.userName} (POST /QuickConnect/Authorize)`)
    } else {
      log(`QUICK CONNECT CODE ${code} for ${server.properties.value}: approve it in Jellyfin (user menu → Quick Connect) within ${Math.round(timeout / 60000)} min`)
    }
    await find({ role: "link", name: "Settings" }, { timeout })
    const status = await api("/api/status")
    if (status.body?.authenticated !== true) throw new Error(`Quick Connect finished but /api/status reports ${JSON.stringify(status.body?.authenticated)}`)
    log(`signed in with Quick Connect as ${status.body.userName}`)
    return status.body
  }

  // GET on the Jellyfin server as the cached account (`--account` only), to
  // read back what the app reported: `/Users/<ctx.account.userId>/Items/<id>`.
  async function jellyfin(apiPath) {
    if (!cachedAccount) throw new Error("ctx.jellyfin needs --account SERVER")
    return jellyfinGet(cachedAccount, apiPath)
  }

  // The token the app holds right now (`--account … --signed-out` only), as
  // a handle that outlives the app forgetting it: `status()` is the HTTP
  // status of GET /Users/Me with it, `revoke()` signs it out on the server
  // from outside, as another device or an administrator would.
  function appSession() {
    if (!account?.signedOut) throw new Error("ctx.appSession needs --account SERVER --signed-out")
    const stored = readStoredSession(dataDir)
    if (!stored?.token) throw new Error("the app holds no token")
    rememberSecret(stored.token)
    const { server_url: serverUrl, token } = stored
    return {
      status: async () => {
        const status = await tokenStatus(serverUrl, token)
        log(`the app's token answers HTTP ${status} on GET /Users/Me`)
        return status
      },
      revoke: async () => {
        await revokeToken(serverUrl, token)
        log("revoked the app's token on the server (POST /Sessions/Logout from outside the app)")
      },
    }
  }

  // Puts a test item back once the app has exited, whatever happened in the
  // drive: a run that fails mid-playback still reports its stop at teardown.
  // The drive's own user-path cleanup comes first; this only acts if the
  // server still shows the item played or with a resume point.
  function restoreItemAfterRun(itemId) {
    if (!cachedAccount) throw new Error("restoreItemAfterRun needs --account SERVER")
    afterRunTasks.push(async () => {
      const { UserData: data } = await jellyfinGet(cachedAccount, `/Items/${itemId}?userId=${cachedAccount.userId}`)
      if (!data.Played && !data.PlaybackPositionTicks) {
        log(`after run: ${itemId} is unplayed without a resume point`)
        return
      }
      await resetUserData(cachedAccount, itemId)
      log(`after run: reset ${itemId} (was played=${data.Played}, position=${data.PlaybackPositionTicks / 1e7} s)`)
    })
  }

  // Cancels a Seerr request once the app has exited if the drive left it
  // pending (its own Cancel failed or never ran).
  function cancelSeerrRequestAfterRun(requestId) {
    if (!cachedAccount) throw new Error("cancelSeerrRequestAfterRun needs --account SERVER")
    afterRunTasks.push(async () => {
      const { results = [] } = await jellyfinGet(cachedAccount, "/MediaFlick/seerr/requests?filter=all&take=100&skip=0")
      if (!results.some((request) => request.id === requestId)) {
        log(`after run: Seerr request ${requestId} is gone`)
        return
      }
      await cancelSeerrRequest(cachedAccount, requestId)
      log(`after run: cancelled the leftover Seerr request ${requestId}`)
    })
  }

  // Runs the after-run tasks; returns their errors.
  async function runAfterRun() {
    const errors = []
    for (const task of afterRunTasks) {
      try {
        await task()
      } catch (error) {
        errors.push(error.message)
        log(`after run failed: ${error.message}`)
      }
    }
    return errors
  }

  // Caches the signed-in session for `--account` runs (`just verify-login`).
  function saveAccount() {
    if (!account?.login) throw new Error("saveAccount needs `just verify-login SERVER`")
    const { account: saved, file } = cacheAccount(account.server, dataDir)
    rememberSecret(saved.token)
    writeEvidence("account.json", { mode: "login", cache: file, ...publicAccount(saved) })
    log(`cached the session for ${saved.userName} on ${saved.server} in ${file}`)
    return publicAccount(saved)
  }

  // The close request a window manager sends for the title bar's close
  // button. Also reports whether the window asked to be minimized within
  // `watchMs`, as a window manager would see it.
  function wmClose({ watchMs = 2000 } = {}) {
    const sent = sendWindowClose(watchMs)
    log(`wm close sent to window ${sent.window}; iconify requested: ${sent.iconifyRequested}`)
    return sent
  }

  // Resolves to { code, signal } once the app exits, or null after `timeout`.
  async function appExit(timeout) {
    let timer
    const exit = await Promise.race([currentChild().exited, new Promise((resolve) => { timer = setTimeout(() => resolve(null), timeout) })])
    clearTimeout(timer)
    log(exit ? `app exited (code ${exit.code}, signal ${exit.signal})` : `app still running after ${timeout} ms`)
    return exit
  }

  // Closes the app through its own close path and starts it again on the same
  // profile, as a user quitting and reopening MediaFlick.
  async function relaunch() {
    app = await relaunchApp()
  }

  async function proxy({ upstream, fail }) {
    const started = await startProxy({ upstream, fail, log })
    proxies.push(started)
    return started
  }

  async function dispose() {
    mpvClient?.socket.destroy()
    await Promise.all(proxies.map((started) => started.close()))
  }

  return {
    get app() {
      return app
    },
    evidence,
    configDir,
    dataDir,
    step,
    check,
    until,
    find,
    count,
    exists,
    state,
    press,
    hover,
    park,
    key,
    fill,
    choose,
    screenshot,
    snapshot,
    api,
    pathname,
    configFile,
    readConfig,
    configStat,
    appLog,
    writeEvidence,
    signIn,
    quickConnect,
    saveAccount,
    jellyfin,
    appSession,
    restoreItemAfterRun,
    cancelSeerrRequestAfterRun,
    runAfterRun,
    player,
    mpv,
    playstates,
    revealPlayer,
    displayCapture,
    account,
    wmClose,
    appExit,
    relaunch,
    windowShown,
    proxy,
    dispose,
    sleep,
  }
}

export function stepLogger(file) {
  return (text) => {
    const line = `${new Date().toISOString()} ${text}`
    appendFileSync(file, `${line}\n`)
    console.log(line)
  }
}
