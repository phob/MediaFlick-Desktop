import { execFileSync } from "node:child_process"

const KEYS = [
  ["XF86Back", "BrowserBack"],
  ["XF86Forward", "BrowserForward"],
  ["XF86HomePage", "BrowserHome"],
  ["Menu", "ContextMenu"],
  ["XF86AudioPlay", "MediaPlayPause"],
  ["XF86AudioPause", "MediaPlayPause"],
  ["XF86AudioStop", "MediaStop"],
  ["XF86AudioNext", "MediaTrackNext"],
  ["XF86AudioPrev", "MediaTrackPrevious"],
  ["Right", "ArrowRight"],
]

export default async function (ctx) {
  await ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })
  await ctx.app.evaluate(`(() => {
    window.__keys = []
    window.addEventListener("keydown", (e) => { window.__keys.push({ key: e.key, keyCode: e.keyCode }); e.preventDefault() }, true)
  })()`)
  const tree = execFileSync("xwininfo", ["-root", "-children"], { encoding: "utf8" })
  ctx.writeEvidence("x11-windows.txt", tree)
  const window = [...tree.matchAll(/^\s+(0x[0-9a-f]+) .*?(\d+)x(\d+)\+/gm)]
    .map(([, id, width, height]) => ({ id, area: Number(width) * Number(height) }))
    .sort((a, b) => b.area - a.area)[0].id
  execFileSync("xdotool", ["mousemove", "--window", window, "640", "400"])
  execFileSync("xdotool", ["windowfocus", "--sync", window])
  const results = []
  for (const [keysym, expected] of KEYS) {
    const before = await ctx.app.evaluate("window.__keys.length")
    execFileSync("xdotool", ["key", keysym])
    await ctx.until(async () => (await ctx.app.evaluate("window.__keys.length")) > before, `${keysym} to reach the page`, 3000, { quiet: true }).catch(() => {})
    const seen = await ctx.app.evaluate(`window.__keys.slice(${before})`)
    results.push({ keysym, expected, seen, pass: seen.some((event) => event.key === expected) })
  }
  ctx.writeEvidence("remote-keys.json", JSON.stringify({ display: process.env.DISPLAY, window, results }, null, 2))
  const failed = results.filter((result) => !result.pass)
  if (failed.length) throw new Error(`keys not delivered as expected: ${JSON.stringify(failed)}`)
}
