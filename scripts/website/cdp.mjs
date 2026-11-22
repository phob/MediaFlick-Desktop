export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

export class CdpClosedError extends Error {
  constructor(message) {
    super(message)
    this.name = "CdpClosedError"
  }
}

export async function connect(port, accept = (target) => target.url.startsWith("mediaflick-desktop://app")) {
  let page
  for (let attempt = 0; attempt < 120 && !page; attempt++) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
      page = targets.find((target) => target.type === "page" && accept(target))
    } catch {
    }
    if (!page) await sleep(500)
  }
  if (!page) throw new Error(`no matching page on CDP port ${port}`)

  const socket = new WebSocket(page.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    socket.onopen = resolve
    socket.onerror = () => reject(new Error(`CDP socket error on port ${port}`))
    socket.onclose = (event) => reject(new CdpClosedError(`CDP socket closed before open (${event.code})`))
  })
  socket.onopen = null
  let nextId = 0
  let closedReason = null
  const pending = new Map()
  const listeners = new Map()

  function failPending(reason) {
    if (closedReason !== null) return
    closedReason = reason
    for (const waiter of pending.values()) {
      waiter.reject(new CdpClosedError(`${waiter.method}: ${reason}`))
    }
    pending.clear()
  }

  socket.onerror = () => failPending("CDP socket error")
  socket.onclose = (event) => failPending(event.reason ? `CDP socket closed (${event.code}: ${event.reason})` : `CDP socket closed (${event.code})`)
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
    if (closedReason !== null || socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new CdpClosedError(`${method}: ${closedReason ?? "CDP socket is not open"}`))
    }
    const id = ++nextId
    return new Promise((resolve, reject) => {
      try {
        socket.send(JSON.stringify({ id, method, params }))
      } catch (error) {
        reject(new CdpClosedError(`${method}: ${error instanceof Error ? error.message : String(error)}`))
        return
      }
      pending.set(id, { resolve, reject, method })
    })
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
      let value
      try {
        value = await evaluate(expression)
      } catch (error) {
        if (error instanceof CdpClosedError) throw error
        value = false
      }
      if (value) return value
      await sleep(interval)
    }
    throw new Error(`timed out waiting for ${expression}`)
  }

  async function route(path) {
    await evaluate(`(history.pushState(null, "", ${JSON.stringify(path)}), dispatchEvent(new PopStateEvent("popstate")), true)`)
  }

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

  function close() {
    failPending("CDP socket closed")
    if (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN) {
      socket.close()
    }
  }

  return { send, on, evaluate, waitFor, route, type, mouse, center, click, hover, close }
}
