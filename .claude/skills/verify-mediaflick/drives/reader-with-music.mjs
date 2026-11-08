// Reading a book while an album plays, on the Built-in player under Linux:
// real X11 media keys pressed with the focus inside the book's iframe must
// pause and resume the music, while Next/Previous track turn the book's
// chapters and leave the queue where it is; the remote's Back leaves the reader
// and the music keeps playing. Writes the test user's playstate and puts it back.
//   just verify reader-with-music --account SERVER --timeout 600
import { execFileSync } from "node:child_process"

const ALBUM = "Test Tones"
const ALBUM_ARTIST = "MediaFlick Fixtures"
const BOOK = "The Adventures of Sherlock Holmes"

const phase = (id) => `document.querySelector('[data-reader-book="${id}"]')?.dataset.readerPhase`
const footer = `[...document.querySelectorAll("[data-reader-footer] > span")].map((span) => span.innerText).join(" · ")`

function appWindow() {
  const tree = execFileSync("xwininfo", ["-root", "-children"], { encoding: "utf8" })
  return [...tree.matchAll(/^\s+(0x[0-9a-f]+) .*?(\d+)x(\d+)\+/gm)]
    .map(([, id, width, height]) => ({ id, area: Number(width) * Number(height) }))
    .sort((a, b) => b.area - a.area)[0].id
}

export default async function readerWithMusic(ctx) {
  if (process.platform !== "linux") throw new Error("reader-with-music sends X11 keys; it runs on Linux only")
  await ctx.signIn({})
  await ctx.audioSink()
  const report = { checks: [], steps: [] }
  const check = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` ${JSON.stringify(detail)}`}`)
  }
  const audio = async () => {
    const player = await ctx.player()
    return player.active && player.nowPlaying?.kind === "audio" ? { index: player.nowPlaying.index, title: player.nowPlaying.current.title, paused: player.paused, phase: player.nowPlaying.phase } : null
  }
  const x = (...args) => execFileSync("xdotool", args)

  const album = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&IncludeItemTypes=MusicAlbum&Recursive=true&SearchTerm=${encodeURIComponent(ALBUM)}`))
    .Items.find((item) => item.Name === ALBUM && item.AlbumArtist === ALBUM_ARTIST)
  const book = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&IncludeItemTypes=Book&Recursive=true&SearchTerm=${encodeURIComponent("Sherlock")}`))
    .Items.find((item) => item.Name === BOOK)
  if (!album || !book) throw new Error(`fixtures missing: album ${Boolean(album)}, book ${Boolean(book)}`)
  const tracks = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&ParentId=${album.Id}&IncludeItemTypes=Audio&Recursive=true`)).Items
  for (const track of tracks) ctx.restoreItemAfterRun(track.Id)
  ctx.restoreItemAfterRun(book.Id)

  await ctx.press({ role: "link", name: "Music" })
  await ctx.press({ role: "link", name: `Open details for ${ALBUM}` }, { nth: 0 })
  await ctx.press({ role: "button", name: "Play" })
  await ctx.until(async () => (await audio())?.phase === "playing", "the album to play", 30000)
  const startIndex = (await audio()).index

  await ctx.press({ role: "link", name: "Books" })
  await ctx.press({ role: "link", name: `Open details for ${BOOK}` })
  await ctx.until(`document.activeElement?.getAttribute("href")?.startsWith("/read/")`, "the book page focuses its read action")
  const window = appWindow()
  x("mousemove", "--window", window, "1270", "10")
  x("windowfocus", "--sync", window)
  x("key", "Return")
  await ctx.until(`${phase(book.Id)} === "ready"`, "Return opens the book", 120000)
  check((await audio())?.paused === false, "the music keeps playing while the book opens")
  x("mousemove", "--window", window, "640", "380", "click", "1")
  const focus = await ctx.until(`document.activeElement?.tagName === "FOLIATE-VIEW" && document.activeElement.tagName`, "a click puts the focus inside the book's iframe")
  report.steps.push({ focus })
  await ctx.screenshot("01-reading-with-music")

  x("key", "XF86AudioPlay")
  const paused = await ctx.until(async () => (await audio())?.paused === true, "XF86AudioPlay inside the book pauses the music", 10000).catch(() => false)
  check(paused, "Play/Pause inside the book's iframe pauses the music")
  x("key", "XF86AudioPlay")
  const resumed = await ctx.until(async () => (await audio())?.paused === false, "XF86AudioPlay again resumes the music", 10000).catch(() => false)
  check(resumed, "Play/Pause again resumes the music")

  const location = async () => { const text = await ctx.app.evaluate(footer); const match = /^(.*?) · Location (\d+) of/.exec(text); return match && { text, chapter: match[1], at: Number(match[2]) } }
  const chapterBefore = await ctx.until(location, "the footer to name the chapter and location", 10000)
  const indexBefore = (await audio()).index
  x("key", "XF86AudioNext")
  const chapterAfter = await ctx.until(async () => { const now = await location(); return now && now.at >= chapterBefore.at && now.chapter !== chapterBefore.chapter && now.chapter !== "" && now }, "XF86AudioNext moves the book to its next chapter", 10000).catch(() => null)
  check(Boolean(chapterAfter), "Next track inside the book turns to the next chapter", { before: chapterBefore, after: chapterAfter })
  const indexAfter = (await audio())?.index
  check(indexAfter === indexBefore, "Next track inside the book leaves the music queue where it was", { before: indexBefore, after: indexAfter })
  report.steps.push({ chapterBefore, chapterAfter, indexBefore, indexAfter, startIndex })

  x("key", "XF86Back")
  const left = await ctx.until(async () => (await ctx.pathname()) === `/item/${book.Id}`, "the remote's Back leaves the reader", 10000).catch(() => false)
  check(left, "Back leaves the reader for the book's page")
  check((await audio())?.paused === false, "the music still plays after leaving the reader")
  await ctx.screenshot("02-back-on-book-page")

  await ctx.press({ role: "button", name: "Stop" })
  await ctx.until(async () => (await audio()) === null, "Stop ends the music")
  ctx.writeEvidence("reader-with-music.json", JSON.stringify(report, null, 2))
  const failed = report.checks.filter((entry) => !entry.ok)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.map((entry) => entry.message).join("; ")}`)
}
