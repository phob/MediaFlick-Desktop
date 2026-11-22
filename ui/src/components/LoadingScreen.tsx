import { useEffect, useState } from "react"
import appIcon from "../../../distribution/app-icon.png?inline"
import { signalWindowReady, windowRevealed } from "@/lib/startup"

const HIDDEN_PAINT_FALLBACK_MS = 100

function nextPaint() {
  if (!windowRevealed() || document.visibilityState === "hidden") return Promise.resolve()
  return new Promise<void>((resolve) => {
    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      window.clearTimeout(fallback)
      resolve()
    }
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
    signalWindowReady()
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
