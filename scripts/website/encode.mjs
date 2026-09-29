// Turns the raw captures from capture.mjs into the website's media.
//
//   node scripts/website/encode.mjs
//
// Stills become WebP at two widths. Screencasts, which arrive as JPEG frames
// with their own timestamps, become H.264 MP4 and VP9 WebM loops with a WebP
// poster. The player clip is composited from the app's transparent overlay
// frames over the same seconds of the source film. Everything is written to
// website/public/media, and build/website-capture/encode-report.json lists
// each output with its size and dimensions. Needs ffmpeg and ffprobe on PATH.

import { mkdir, readFile, writeFile, stat, rm } from "node:fs/promises"
import { existsSync } from "node:fs"
import { spawnSync } from "node:child_process"
import path from "node:path"

const root = path.resolve(import.meta.dirname, "../..")
const rawDir = path.join(root, "build", "website-capture", "raw")
const workDir = path.join(root, "build", "website-capture", "work")
const outDir = path.join(root, "website", "public", "media")

const STILLS = [
  "signin", "home", "home-full", "movies", "movie-detail", "series-detail", "search", "appearance",
  "timeline", "calendar", "discover", "requests", "collections", "rated-detail",
]
const STILL_WIDTHS = [1600, 960]
const VIDEO_WIDTH = 1280
const HERO_WIDTH = 1600

function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 })
  if (result.status !== 0) throw new Error(`${command} ${args.slice(0, 6).join(" ")} … failed:\n${result.stderr}`)
  return result.stdout
}
const ffmpeg = (args) => run("ffmpeg", ["-loglevel", "error", "-y", ...args])

function dimensions(file) {
  const [width, height] = run("ffprobe", ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0", file]).trim().split(",").map(Number)
  return { width, height }
}

function duration(file) {
  return Number(run("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", file]).trim())
}

const outputs = []
async function note(file, kind) {
  const info = { file: path.relative(root, file).replaceAll("\\", "/"), kind, bytes: (await stat(file)).size, ...dimensions(file) }
  if (kind === "video") info.seconds = Math.round(duration(file) * 100) / 100
  outputs.push(info)
}

// A concat-demuxer list that holds every frame for its own duration. The
// demuxer ignores the last entry's duration, so that frame is listed twice.
async function frameList(dir, frames, name) {
  const lines = ["ffconcat version 1.0"]
  frames.forEach((frame, index) => {
    lines.push(`file '${path.join(dir, frame.file).replaceAll("\\", "/")}'`, `duration ${frame.duration.toFixed(4)}`)
    if (index === frames.length - 1) lines.push(`file '${path.join(dir, frame.file).replaceAll("\\", "/")}'`)
  })
  const list = path.join(workDir, `${name}.ffconcat`)
  await writeFile(list, `${lines.join("\n")}\n`)
  return list
}

// Screencast frames carry wall-clock timestamps; the last frame is held until
// the recording stopped. A `from` mark cuts everything before it, and the
// frame on screen at that moment becomes the first.
async function screencastList(name) {
  const dir = path.join(rawDir, name)
  const { stopped, from, frames: all } = JSON.parse(await readFile(path.join(dir, "frames.json"), "utf8"))
  const start = typeof from === "number" ? from : -Infinity
  const frames = all.filter((frame, index) => (all[index + 1]?.time ?? stopped) > start)
  const timed = frames.map((frame, index) => ({
    file: frame.file,
    duration: Math.max(1 / 60, (frames[index + 1]?.time ?? stopped) - Math.max(frame.time, start)),
  }))
  return frameList(dir, timed, name)
}

function encodeLoop(input, filter, name, width) {
  const mp4 = path.join(outDir, `${name}.mp4`)
  const webm = path.join(outDir, `${name}.webm`)
  const poster = path.join(outDir, `${name}-poster.webp`)
  const scale = `${filter ? `${filter},` : ""}scale=${width}:-2:flags=lanczos,fps=30,format=yuv420p`
  ffmpeg([...input, "-vf", scale, "-an", "-c:v", "libx264", "-preset", "slow", "-crf", "23", "-profile:v", "high", "-movflags", "+faststart", mp4])
  ffmpeg(["-i", mp4, "-an", "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "36", "-row-mt", "1", "-deadline", "good", "-cpu-used", "2", webm])
  ffmpeg(["-i", mp4, "-frames:v", "1", "-c:v", "libwebp", "-quality", "80", poster])
  return { mp4, webm, poster }
}

// The film under the app's own overlay, frame for frame: each overlay frame
// was captured at a known film position, and the source is cut to start at
// the first of them.
async function compositePlayer() {
  const report = JSON.parse(await readFile(path.join(rawDir, "report-demo.json"), "utf8"))
  const { source, overlay } = report.scenes.player ?? {}
  if (!source || !overlay) throw new Error("run `capture.mjs demo player` first")
  const { frames } = JSON.parse(await readFile(path.join(overlay, "frames.json"), "utf8"))
  const start = frames[0].positionMs
  const end = frames.at(-1).positionMs + 80
  const timed = frames.map((frame, index) => ({ file: frame.file, duration: ((frames[index + 1]?.positionMs ?? end) - frame.positionMs) / 1000 }))
  const list = await frameList(overlay, timed, "player-overlay")
  const { width, height } = dimensions(path.join(overlay, frames[0].file))
  const clip = path.join(workDir, "player.mkv")
  ffmpeg([
    "-ss", String(start / 1000), "-t", String((end - start) / 1000), "-i", source,
    "-f", "concat", "-safe", "0", "-i", list,
    "-filter_complex", `[0:v]scale=${width}:${height}:flags=lanczos,setpts=PTS-STARTPTS[film];[1:v]setpts=PTS-STARTPTS[ui];[film][ui]overlay=0:0:eof_action=repeat,fps=30,format=yuv444p`,
    "-an", "-c:v", "libx264", "-qp", "0", "-preset", "ultrafast", clip,
  ])
  const still = path.join(workDir, "player.png")
  ffmpeg(["-ss", String(((end - start) / 1000) * 0.55), "-i", clip, "-frames:v", "1", still])
  return { clip, still, width, height }
}

await rm(workDir, { recursive: true, force: true })
await mkdir(workDir, { recursive: true })
await mkdir(outDir, { recursive: true })

for (const name of STILLS) {
  const source = path.join(rawDir, `${name}.png`)
  if (!existsSync(source)) throw new Error(`missing still ${name}.png; run capture.mjs first`)
  for (const width of STILL_WIDTHS) {
    const file = path.join(outDir, width === STILL_WIDTHS[0] ? `${name}.webp` : `${name}-${width}.webp`)
    ffmpeg(["-i", source, "-vf", `scale=${width}:-2:flags=lanczos`, "-c:v", "libwebp", "-quality", "82", "-compression_level", "6", file])
    await note(file, "still")
  }
}

for (const name of ["home-hover", "search"]) {
  const list = await screencastList(name)
  const files = encodeLoop(["-f", "concat", "-safe", "0", "-i", list], "", name, VIDEO_WIDTH)
  for (const [kind, file] of Object.entries(files)) await note(file, kind === "poster" ? "still" : "video")
}

const player = await compositePlayer()
for (const width of STILL_WIDTHS) {
  const file = path.join(outDir, width === STILL_WIDTHS[0] ? "player.webp" : `player-${width}.webp`)
  ffmpeg(["-i", player.still, "-vf", `scale=${width}:-2:flags=lanczos`, "-c:v", "libwebp", "-quality", "82", file])
  await note(file, "still")
}
for (const [kind, file] of Object.entries(encodeLoop(["-i", player.clip], "", "player", VIDEO_WIDTH))) {
  await note(file, kind === "poster" ? "still" : "video")
}

// Lossless intermediates stay 4:4:4: the window's 1125-pixel height is odd,
// which 4:2:0 cannot hold. Only the final encodes scale to even sizes.

// Hero: the browsing tour, cropped from the 16:10 window to the player's
// 16:9 frame, cross-fading into the composited playback.
const tourList = await screencastList("tour")
const tourClip = path.join(workDir, "tour.mkv")
ffmpeg([
  "-f", "concat", "-safe", "0", "-i", tourList,
  "-vf", `crop=iw:iw*${player.height}/${player.width}:0:0,scale=${player.width}:${player.height}:flags=lanczos,fps=30,format=yuv444p`,
  "-c:v", "libx264", "-qp", "0", "-preset", "ultrafast", tourClip,
])
const fade = 0.5
const heroClip = path.join(workDir, "hero.mkv")
ffmpeg([
  "-i", tourClip, "-i", player.clip,
  "-filter_complex", `[0:v][1:v]xfade=transition=fade:duration=${fade}:offset=${(duration(tourClip) - fade).toFixed(3)},format=yuv444p`,
  "-c:v", "libx264", "-qp", "0", "-preset", "ultrafast", heroClip,
])
for (const [kind, file] of Object.entries(encodeLoop(["-i", heroClip], "", "hero", HERO_WIDTH))) {
  await note(file, kind === "poster" ? "still" : "video")
}

const report = path.join(root, "build", "website-capture", "encode-report.json")
// The page's hero readouts switch from the library ones to the playback ones
// at this second; it belongs in data-play-at on the hero video in index.html.
const heroPlayAt = Math.round((duration(tourClip) - fade / 2) * 10) / 10
await writeFile(report, JSON.stringify({ encodedAt: new Date().toISOString(), heroPlayAt, outputs }, null, 2))
console.log(`hero playback starts at ${heroPlayAt}s; set data-play-at on the hero video in website/public/index.html`)
const total = outputs.reduce((sum, output) => sum + output.bytes, 0)
console.log(`${outputs.length} files, ${(total / 1024 / 1024).toFixed(1)} MiB → ${path.relative(root, outDir)}`)
console.log(`report: ${path.relative(root, report)}`)
