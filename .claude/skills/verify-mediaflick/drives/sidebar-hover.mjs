import { execFileSync } from "node:child_process"

const DEMO = "https://demo.jellyfin.org/stable"
const LIBRARY_ROUTE = /^\/library\/[0-9a-f]+$/
const STEP_MS = 20
const AWAY_MS = 10
const BOUNCE_STEP_PX = 4

const xdotool = (...args) => execFileSync("xdotool", args.map(String), { encoding: "utf8" })
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const sidebarState = (ctx) => ctx.app.evaluate(`document.querySelector('[data-slot="sidebar"]')?.dataset.state ?? null`)

function appWindow() {
  const tree = execFileSync("xwininfo", ["-root", "-children"], { encoding: "utf8" })
  return [...tree.matchAll(/^\s+(0x[0-9a-f]+) .*?(\d+)x(\d+)\+/gm)]
    .map(([, id, width, height]) => ({ id, area: Number(width) * Number(height) }))
    .sort((a, b) => b.area - a.area)[0].id
}

async function glide(window, from, to) {
  const steps = Math.max(1, Math.round(Math.max(Math.abs(to.x - from.x), Math.abs(to.y - from.y))))
  for (let step = 1; step <= steps; step++) {
    const t = step / steps
    xdotool("mousemove", "--window", window, Math.round(from.x + (to.x - from.x) * t), Math.round(from.y + (to.y - from.y) * t))
    await pause(STEP_MS)
  }
}

const PROBE = `(() => {
  const sidebar = document.querySelector('[data-slot="sidebar"]')
  const container = document.querySelector('[data-slot="sidebar-container"]')
  const t0 = performance.now()
  const now = () => Math.round(performance.now() - t0)
  const name = (node) => {
    if (!node) return null
    if (node.nodeType !== 1) return node.nodeName
    const slot = node.dataset.slot ?? node.dataset.sidebar
    const text = node.closest('[data-sidebar="menu-button"]')?.textContent?.trim()
    return node.tagName.toLowerCase() + (slot ? "[" + slot + "]" : "") + (text ? " (" + text + ")" : "")
  }
  const log = []
  for (const type of ["pointerover", "pointerout", "pointerenter", "pointerleave"]) {
    document.addEventListener(type, (event) => {
      const crossing = type === "pointerover" || type === "pointerout"
      if (!crossing && event.target !== container) return
      if (crossing && event.relatedTarget && container.contains(event.target) === container.contains(event.relatedTarget)) return
      log.push({ t: now(), type, target: name(event.target), related: name(event.relatedTarget), x: Math.round(event.clientX), y: Math.round(event.clientY) })
    }, true)
  }
  new MutationObserver(() => log.push({ t: now(), type: "state", state: sidebar.dataset.state })).observe(sidebar, { attributes: true, attributeFilter: ["data-state"] })
  window.__sidebarProbe = { log, mark: (label) => log.push({ t: now(), type: "mark", label }) }
  return true
})()`

async function pass(ctx, label, move) {
  await ctx.app.evaluate(`window.__sidebarProbe.mark(${JSON.stringify(label)})`)
  await move()
  await ctx.app.evaluate(`window.__sidebarProbe.mark("end")`)
  const log = JSON.parse(await ctx.app.evaluate(`JSON.stringify(window.__sidebarProbe.log)`))
  const events = log.slice(log.findLastIndex((entry) => entry.label === label))
  const collapses = events.filter((entry) => entry.type === "state" && entry.state === "collapsed").length
  ctx.step(`${label}: ${collapses} collapses, ${events.filter((entry) => entry.type === "pointerout" && entry.related === null).length} pointerouts to nothing`)
  return { pass: label, collapses, events }
}

export default async function sidebarHover(ctx) {
  if (process.platform !== "linux") throw new Error("sidebar-hover moves the X11 pointer with xdotool and runs on Linux only")
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.press({ role: "link", name: "Movies" })
  await ctx.until(async () => LIBRARY_ROUTE.test(await ctx.pathname()), "the Movies library")

  const window = appWindow()
  const away = { x: 700, y: 300 }
  xdotool("mousemove", "--window", window, away.x, away.y)
  await ctx.until(async () => (await sidebarState(ctx)) === "collapsed", "the sidebar collapsed with the pointer on the page")
  const rows = JSON.parse(await ctx.app.evaluate(`JSON.stringify([...document.querySelectorAll('[data-sidebar="menu-button"]')].map((button) => {
    const rect = button.getBoundingClientRect()
    return { name: button.textContent.trim(), x: rect.x, y: rect.y, width: rect.width, height: rect.height }
  }))`))
  const first = rows[0]
  const last = rows.at(-1)
  const icons = { x: Math.round(first.x + first.width / 2), top: Math.round(first.y + first.height / 2), bottom: Math.round(last.y + last.height / 2) }
  const labels = { x: icons.x + 90 }
  const [innerWidth] = await ctx.app.evaluate("[innerWidth, innerHeight]")
  const outside = { x: innerWidth + 200, y: 300 }
  await ctx.app.evaluate(PROBE)
  await ctx.screenshot("01-collapsed")

  await glide(window, away, { x: icons.x, y: icons.top })
  await ctx.until(async () => (await sidebarState(ctx)) === "expanded", "the sidebar expanded under the real pointer")
  await pause(600)
  await ctx.screenshot("02-expanded")
  const passes = [
    await pass(ctx, "down-icons", () => glide(window, { x: icons.x, y: icons.top }, { x: icons.x, y: icons.bottom })),
    await pass(ctx, "up-labels", async () => {
      await glide(window, { x: icons.x, y: icons.bottom }, { x: labels.x, y: icons.bottom })
      await glide(window, { x: labels.x, y: icons.bottom }, { x: labels.x, y: icons.top })
    }),
    await pass(ctx, "down-icons-leaving-the-window", async () => {
      for (let y = icons.top; y <= icons.bottom; y += BOUNCE_STEP_PX) {
        xdotool("mousemove", "--window", window, outside.x, outside.y)
        await pause(AWAY_MS)
        xdotool("mousemove", "--window", window, icons.x, y)
        await pause(STEP_MS)
      }
    }),
  ]
  await ctx.screenshot("03-still-expanded")
  await glide(window, { x: labels.x, y: icons.top }, away)
  await ctx.until(async () => (await sidebarState(ctx)) === "collapsed", "the sidebar collapsed once the pointer left it")
  await ctx.screenshot("04-collapsed-again")

  ctx.writeEvidence("sidebar-hover.json", { rows, icons, labels, outside, stepMs: STEP_MS, awayMs: AWAY_MS, passes })
  ctx.check(
    passes.every((entry) => entry.collapses === 0),
    `the sidebar stays expanded while the pointer moves down its icons, up its labels, and down again leaving the window for ${AWAY_MS} ms each step (${passes.map((entry) => `${entry.pass}: ${entry.collapses} collapses`).join(", ")})`,
  )
}
