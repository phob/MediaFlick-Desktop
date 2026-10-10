// The `ctx` a drive receives. Elements are found the way assistive technology
// sees them (CDP Accessibility.queryAXTree: role + accessible name) and
// operated with real CDP mouse and keyboard input, so React sees the same
// events a user produces.

import { appendFileSync, existsSync, readFileSync, statSync, writeFileSync } from "node:fs"
import path from "node:path"
import { sleep } from "../../../../scripts/website/cdp.mjs"

const SKIPPED_ROLES = new Set(["InlineTextBox", "LineBreak", "none", "generic", "ignored"])
const STATE_PROPERTIES = ["focused", "disabled", "checked", "pressed", "expanded", "selected", "invalid", "required", "readonly"]

export function createHarness({ app, evidence, configDir, dataDir, log }) {
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
  async function state(target) {
    const [node] = await axQuery(target)
    if (!node) throw new Error(`no element ${describe(target)}`)
    return props(node)
  }

  async function mouseAt(x, y) {
    const event = { x, y, button: "left", clickCount: 1 }
    await app.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "none" })
    await app.send("Input.dispatchMouseEvent", { type: "mousePressed", ...event })
    await app.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...event })
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

  // Scrolls the element into view and returns a point the pointer really hits.
  // The element must hold still first: leaving Home turns the pinned sidebar
  // into a collapsed overlay and slides the content left, and a click at the
  // old position lands on nothing. A toast covering it is waited out. Any
  // other covered target first gets time to settle (transitions end, the
  // expanding sidebar finishes); only then is the pointer parked, because
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
          await sleep(300)
          continue
        }
        covered++
        if (covered === 12) throw new Error(`${describe(target)} is covered by ${hit}`)
        if (covered === 6) await park()
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

  function writeEvidence(name, value) {
    writeFileSync(path.join(evidence, name), typeof value === "string" ? value : `${JSON.stringify(value, null, 2)}\n`)
    log(`evidence ${name}`)
  }

  // Signs in through the sign-in form. With no server given, the field must
  // already hold one (pass `--url` to the session).
  async function signIn({ server, username, password = "" }) {
    await find({ role: "button", name: "Sign in" }, { timeout: 30000 })
    if (server) await fill({ role: "textbox", name: "Server" }, server)
    await fill({ role: "textbox", name: "Username" }, username)
    if (password) await fill({ role: "textbox", name: "Password" }, password)
    await press({ role: "button", name: "Sign in" })
    await find({ role: "link", name: "Settings" }, { timeout: 60000 })
    log(`signed in as ${username}`)
  }

  return {
    app,
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
    writeEvidence,
    signIn,
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
