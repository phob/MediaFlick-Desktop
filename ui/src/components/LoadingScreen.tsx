import { useEffect, useState } from "react"
import appIcon from "../../../distribution/app-icon.png?inline"
import { api } from "@/lib/api"

const HIDDEN_PAINT_FALLBACK_MS = 100

function nextPaint() {
  return new Promise<void>((resolve) => {
    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      window.clearTimeout(fallback)
      resolve()
    }
    // Chromium may pause animation frames while the native window is hidden.
    // Keep the normal two-paint path, but do not deadlock startup before the
    // shell has had its first opportunity to reveal the completed document.
    const fallback = window.setTimeout(finish, HIDDEN_PAINT_FALLBACK_MS)
    requestAnimationFrame(() => requestAnimationFrame(finish))
  })
}

export function LoadingScreen({ ready }: { ready: boolean }) {
  const [mounted, setMounted] = useState(true)

  useEffect(() => {
    if (!ready) return
    let cancelled = false
    void nextPaint().then(() => {
      if (!cancelled) setMounted(false)
    })
    return () => { cancelled = true }
  }, [ready])

  useEffect(() => {
    if (mounted) return

    // Effects run after React commits the cover's removal. The native request
    // is therefore the final startup step, making the finished route—not the
    // loading cover—the main window's first visible frame.
    void api.shell.windowReady().catch(() => {
      // Service initialization failures also take the JSON shell queue down.
      // The exact app-origin-only action keeps the native recovery surface
      // reachable without weakening the authenticated dialog bridge.
      if (window.location.protocol === "mediaflick-desktop:") {
        const beacon = document.createElement("img")
        beacon.hidden = true
        beacon.alt = ""
        beacon.addEventListener("error", () => beacon.remove(), { once: true })
        document.body.append(beacon)
        beacon.src = "mediaflick-desktop://window-ready"
        window.setTimeout(() => beacon.remove(), 1_000)
      }
    })
  }, [mounted])

  if (!mounted) return null

  return (
    <div
      className="loading-screen"
      role="status"
      aria-label="Loading MediaFlick"
    >
      <div className="loading-screen-content">
        <img className="loading-screen-logo" src={appIcon} alt="MediaFlick" />
        <div
          className="loading-screen-progress"
          role="progressbar"
          aria-label="Loading MediaFlick"
          aria-valuetext="Loading"
        >
          <span className="loading-screen-progress-fill" />
        </div>
      </div>
    </div>
  )
}
