// Off Home the sidebar is collapsed to icons and expands under the pointer.
// Expanding must not move any icon, so the control under the pointer stays
// under it.
//   just verify sidebar-stable-icons
const DEMO = "https://demo.jellyfin.org/stable"
const MOVIES = { role: "link", name: "Movies" }
const sidebarState = (ctx) => ctx.app.evaluate(`document.querySelector('[data-slot="sidebar"]')?.dataset.state ?? null`)

const ICONS = `(() => {
  const icons = {}
  for (const button of document.querySelectorAll('[data-sidebar="menu-button"]')) {
    const name = button.querySelector("span")?.textContent?.trim()
    const icon = button.firstElementChild
    if (!name || !icon) continue
    const { x, y } = icon.getBoundingClientRect()
    icons[name] = { x: Math.round(x * 10) / 10, y: Math.round(y * 10) / 10 }
  }
  const search = document.querySelector('[data-sidebar="content"] form > svg')
  if (search) {
    const { x, y } = search.getBoundingClientRect()
    icons.Search = { x: Math.round(x * 10) / 10, y: Math.round(y * 10) / 10 }
  }
  return JSON.stringify(icons)
})()`

async function settledIcons(ctx, description) {
  let previous = null
  let result = null
  await ctx.until(async () => {
    const current = await ctx.app.evaluate(ICONS)
    const settled = current === previous
    previous = current
    if (settled) result = JSON.parse(current)
    return settled
  }, description, 5_000)
  return result
}

export default async function sidebarStableIcons(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.press(MOVIES)
  await ctx.until(async () => (await ctx.pathname()) === "/library?kind=Movie", "the Movies library")
  await ctx.park()
  await ctx.until(async () => (await sidebarState(ctx)) === "collapsed", "the sidebar collapsed off Home")
  const collapsed = await settledIcons(ctx, "collapsed icons at rest")
  await ctx.screenshot("01-collapsed")

  const movies = await ctx.find(MOVIES)
  const pointer = { x: movies.rect.x, y: movies.rect.y }
  await ctx.hover(MOVIES)
  await ctx.until(async () => (await sidebarState(ctx)) === "expanded", "the sidebar expanded under the pointer")
  const expanded = await settledIcons(ctx, "expanded icons at rest")
  await ctx.screenshot("02-expanded")

  const underPointer = await ctx.app.evaluate(
    `document.elementFromPoint(${pointer.x}, ${pointer.y})?.closest('[data-sidebar="menu-button"]')?.textContent?.trim() ?? null`,
  )
  const moves = Object.keys(collapsed).map((name) => ({
    name,
    collapsed: collapsed[name],
    expanded: expanded[name] ?? null,
    dx: expanded[name] ? Math.round((expanded[name].x - collapsed[name].x) * 10) / 10 : null,
    dy: expanded[name] ? Math.round((expanded[name].y - collapsed[name].y) * 10) / 10 : null,
  }))
  await ctx.writeEvidence("sidebar-icons.json", { pointer, underPointer, moves })

  const moved = moves.filter((move) => move.dx === null || Math.abs(move.dx) > 0.5 || Math.abs(move.dy) > 0.5)
  ctx.check(
    moved.length === 0,
    `no sidebar icon moves when the sidebar expands${moved.length ? `; moved: ${moved.map((m) => `${m.name} dx=${m.dx} dy=${m.dy}`).join(", ")}` : ""}`,
  )
  ctx.check(underPointer === "Movies", `Movies stays under the pointer after expanding (found ${underPointer})`)
}
