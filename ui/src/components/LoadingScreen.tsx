import { useEffect, useState } from "react"
import appIcon from "../../../resources/app-icon.png?inline"

const READY_SETTLE_MS = 160
const IMAGE_SETTLE_LIMIT_MS = 5_000
const COMPLETE_HOLD_MS = 180
const FADE_MS = 520

function nextPaint() {
  return new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()))
  })
}

function visibleImages() {
  return Array.from(document.images).filter((image) => {
    const bounds = image.getBoundingClientRect()
    return (
      bounds.width > 0 &&
      bounds.height > 0 &&
      bounds.right > 0 &&
      bounds.bottom > 0 &&
      bounds.left < window.innerWidth &&
      bounds.top < window.innerHeight
    )
  })
}

function waitForImage(image: HTMLImageElement, timeout: number) {
  if (image.complete) return Promise.resolve()

  return new Promise<void>((resolve) => {
    const finish = () => {
      window.clearTimeout(timer)
      image.removeEventListener("load", finish)
      image.removeEventListener("error", finish)
      resolve()
    }
    const timer = window.setTimeout(finish, timeout)
    image.addEventListener("load", finish, { once: true })
    image.addEventListener("error", finish, { once: true })
  })
}

function waitForTimeout(timeout: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, timeout))
}

/**
 * The data can settle a render before its artwork has decoded. Wait for the
 * images that are actually in the first viewport, then give React one more
 * paint to replace any failed image with its fallback.
 */
async function waitForInitialVisuals() {
  const deadline = Date.now() + IMAGE_SETTLE_LIMIT_MS
  await nextPaint()

  while (Date.now() < deadline) {
    const images = visibleImages()
    const pending = images.filter((image) => !image.complete)
    if (!pending.length) {
      const remaining = Math.max(0, deadline - Date.now())
      await Promise.race([
        Promise.allSettled(images.map((image) => image.decode())),
        waitForTimeout(remaining),
      ])
      await nextPaint()
      return
    }

    const remaining = Math.max(0, deadline - Date.now())
    await Promise.all(pending.map((image) => waitForImage(image, remaining)))
    await nextPaint()
  }
}

export function LoadingScreen({ ready }: { ready: boolean }) {
  const [complete, setComplete] = useState(false)
  const [exiting, setExiting] = useState(false)
  const [mounted, setMounted] = useState(true)

  useEffect(() => {
    if (!ready || complete) return

    let cancelled = false
    const settle = window.setTimeout(() => {
      void waitForInitialVisuals().then(() => {
        if (!cancelled) setComplete(true)
      })
    }, READY_SETTLE_MS)

    return () => {
      cancelled = true
      window.clearTimeout(settle)
    }
  }, [complete, ready])

  useEffect(() => {
    if (!complete) return

    const exit = window.setTimeout(() => setExiting(true), COMPLETE_HOLD_MS)
    const unmount = window.setTimeout(() => setMounted(false), COMPLETE_HOLD_MS + FADE_MS)
    return () => {
      window.clearTimeout(exit)
      window.clearTimeout(unmount)
    }
  }, [complete])

  if (!mounted) return null

  return (
    <div
      className="loading-screen"
      data-complete={complete}
      data-exiting={exiting}
      role="status"
      aria-label={complete ? "MediaFlick is ready" : "Loading MediaFlick"}
    >
      <div className="loading-screen-content">
        <img className="loading-screen-logo" src={appIcon} alt="MediaFlick" />
        <div
          className="loading-screen-progress"
          role="progressbar"
          aria-label="Loading MediaFlick"
          aria-valuetext={complete ? "Complete" : "Loading"}
        >
          <span className="loading-screen-progress-fill" />
        </div>
      </div>
    </div>
  )
}
