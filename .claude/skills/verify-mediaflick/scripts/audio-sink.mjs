// A private PipeWire with one null sink, inside the session's private runtime
// dir, so mpv has an audio device to play into under Xvfb and the drive can
// record what reached it. The user's own audio server is never contacted: the
// sockets live in the private XDG_RUNTIME_DIR, WirePlumber watches no ALSA,
// Bluetooth or camera devices, and the configs live in a temp dir of their own.

import { execFileSync, spawn, spawnSync } from "node:child_process"
import { copyFileSync, mkdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { sleep } from "../../../../scripts/website/cdp.mjs"

const SINK = "mediaflick-verify-sink"

const SINK_CONFIG = `context.objects = [
  { factory = adapter
    args = {
      factory.name = support.null-audio-sink
      node.name = ${SINK}
      node.description = "MediaFlick verify sink"
      media.class = Audio/Sink
      audio.position = [ FL FR ]
      object.linger = true
    }
  }
]
`

const WIREPLUMBER_CONFIG = `wireplumber.profiles = {
  main = {
    monitor.alsa = disabled
    monitor.alsa-midi = disabled
    monitor.bluez = disabled
    monitor.bluez.seat-monitoring = disabled
    monitor.v4l2 = disabled
    monitor.libcamera = disabled
    support.portal-permissionstore = disabled
    support.reserve-device = disabled
  }
}
`

export async function startAudioSink({ evidence, log }) {
  const runtime = process.env.XDG_RUNTIME_DIR
  if (!runtime?.includes("mediaflick-xvfb.")) throw new Error("audioSink needs the Linux wrapper's private XDG_RUNTIME_DIR")
  const root = path.join(runtime, "audio-sink")
  const pipewireDir = path.join(root, "pipewire")
  const wireplumberDir = path.join(root, "wireplumber")
  mkdirSync(path.join(pipewireDir, "pipewire.conf.d"), { recursive: true })
  mkdirSync(path.join(wireplumberDir, "wireplumber.conf.d"), { recursive: true })
  copyFileSync("/usr/share/pipewire/pipewire.conf", path.join(pipewireDir, "pipewire.conf"))
  writeFileSync(path.join(pipewireDir, "pipewire.conf.d", "verify-sink.conf"), SINK_CONFIG)
  copyFileSync("/usr/share/wireplumber/wireplumber.conf", path.join(wireplumberDir, "wireplumber.conf"))
  writeFileSync(path.join(wireplumberDir, "wireplumber.conf.d", "verify.conf"), WIREPLUMBER_CONFIG)
  const env = { ...process.env }
  delete env.PULSE_SERVER
  delete env.PIPEWIRE_REMOTE
  const children = [spawn("pipewire", [], { env: { ...env, PIPEWIRE_CONFIG_DIR: pipewireDir }, stdio: "ignore" })]
  await sleep(500)
  children.push(spawn("wireplumber", [], { env: { ...env, WIREPLUMBER_CONFIG_DIR: wireplumberDir }, stdio: "ignore" }))
  const deadline = Date.now() + 10000
  while (!execFileSync("pw-cli", ["ls", "Node"], { encoding: "utf8", env }).includes(`node.name = "${SINK}"`)) {
    if (Date.now() > deadline) throw new Error("the private PipeWire sink did not appear within 10 s")
    await sleep(200)
  }
  log(`audio sink: private PipeWire ${SINK} in ${root}`)

  // Records the sink for `seconds` into `<name>.wav` and measures its level
  // with ffmpeg's volumedetect: silence reads about -91 dB.
  async function capture(name, seconds) {
    const file = path.join(evidence, `${name}.wav`)
    const recorder = spawn("pw-record", ["--target", SINK, "-P", "stream.capture.sink=true", file], { env, stdio: "ignore" })
    await sleep(seconds * 1000)
    recorder.kill("SIGINT")
    await new Promise((resolve) => recorder.once("exit", resolve))
    const { stderr } = spawnSync("ffmpeg", ["-hide_banner", "-i", file, "-af", "volumedetect", "-f", "null", "-"], { encoding: "utf8" })
    const level = (key) => Number(stderr.match(new RegExp(`${key}: (-?[\\d.]+) dB`))?.[1] ?? Number.NaN)
    const measured = { file: path.basename(file), seconds, meanDb: level("mean_volume"), maxDb: level("max_volume") }
    log(`audio capture ${measured.file}: mean ${measured.meanDb} dB, max ${measured.maxDb} dB`)
    return measured
  }

  function stop() {
    for (const child of children.reverse()) child.kill("SIGTERM")
  }

  return { sink: SINK, capture, stop }
}
