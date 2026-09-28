// Minimal Chrome DevTools Protocol client for driving a running MediaFlick
// Desktop started with `--remote-debugging-port`. Node's built-in WebSocket is
// enough; no browser automation dependency is needed.

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

// Attaches to the first page target the predicate accepts; by default the
// MediaFlick Desktop app page.
export async function connect(port, accept = (target) => target.url.startsWith("mediaflick-desktop://app")) {
  let page
  for (let attempt = 0; attempt < 120 && !page; attempt++) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
      page = targets.find((target) => target.type === "page" && accept(target))
    } catch {
      // The app is still starting.
    }
    if (!page) await sleep(500)
  }
  if (!page) throw new Error(`no matching page on CDP port ${port}`)

  const socket = new WebSocket(page.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    socket.onopen = resolve
    socket.onerror = reject
  })
  let nextId = 0
  const pending = new Map()
  const listeners = new Map()
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data)
    if (message.method) {
      for (const listener of listeners.get(message.method) ?? []) listener(message.params)
      return
    }
    const waiter = pending.get(message.id)
    if (!waiter) return
    pending.delete(message.id)
    if (message.error) waiter.reject(new Error(`${waiter.method}: ${message.error.message}`))
    else waiter.resolve(message.result)
  }

  function send(method, params = {}) {
    const id = ++nextId
    socket.send(JSON.stringify({ id, method, params }))
    return new Promise((resolve, reject) => pending.set(id, { resolve, reject, method }))
  }

  function on(method, listener) {
    const list = listeners.get(method) ?? []
    list.push(listener)
    listeners.set(method, list)
    return () => listeners.set(method, (listeners.get(method) ?? []).filter((entry) => entry !== listener))
  }

  async function evaluate(expression) {
    const result = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text ?? "evaluation failed")
    }
    return result.result.value
  }

  async function waitFor(expression, { timeout = 20000, interval = 250 } = {}) {
    const deadline = Date.now() + timeout
    while (Date.now() < deadline) {
      const value = await evaluate(expression).catch(() => false)
      if (value) return value
      await sleep(interval)
    }
    throw new Error(`timed out waiting for ${expression}`)
  }

  // Client-side navigation keeps the running app state; a reload would follow
  // the startup-page setting instead of the requested route.
  async function route(path) {
    await evaluate(`(history.pushState(null, "", ${JSON.stringify(path)}), dispatchEvent(new PopStateEvent("popstate")), true)`)
  }

  // Real keyboard input, so React's controlled inputs and debounced search
  // see the same events a user produces.
  async function type(selector, text, { delay = 0, clear = true } = {}) {
    await evaluate(`(() => {
      const element = document.querySelector(${JSON.stringify(selector)})
      if (!element) throw new Error("no element for ${selector.replaceAll('"', '\\"')}")
      element.focus()
      if (${clear}) element.select?.()
      return true
    })()`)
    if (text === "") {
      const key = { key: "Backspace", code: "Backspace", windowsVirtualKeyCode: 8 }
      await send("Input.dispatchKeyEvent", { type: "keyDown", ...key })
      await send("Input.dispatchKeyEvent", { type: "keyUp", ...key })
      return
    }
    if (delay === 0) {
      await send("Input.insertText", { text })
      return
    }
    for (const character of text) {
      await send("Input.insertText", { text: character })
      await sleep(delay)
    }
  }

  async function mouse(type, x, y) {
    await send("Input.dispatchMouseEvent", { type, x, y, button: type === "mouseMoved" ? "none" : "left", clickCount: 1 })
  }

  // Centre of the first element matching the selector, in CSS pixels.
  async function center(selector) {
    return evaluate(`(() => {
      const element = document.querySelector(${JSON.stringify(selector)})
      if (!element) return null
      const rect = element.getBoundingClientRect()
      return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
    })()`)
  }

  async function click(selector) {
    const point = await center(selector)
    if (!point) throw new Error(`no element for ${selector}`)
    await mouse("mouseMoved", point.x, point.y)
    await mouse("mousePressed", point.x, point.y)
    await mouse("mouseReleased", point.x, point.y)
  }

  async function hover(selector) {
    const point = await center(selector)
    if (!point) throw new Error(`no element for ${selector}`)
    await mouse("mouseMoved", point.x, point.y)
  }

  return { send, on, evaluate, waitFor, route, type, mouse, center, click, hover, close: () => socket.close() }
}
