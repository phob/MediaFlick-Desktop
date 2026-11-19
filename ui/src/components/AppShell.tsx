import { useViewing } from "@/lib/viewing"
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react"
import { matchPath, useLocation } from "react-router-dom"
import { AppSidebar } from "@/components/AppSidebar"
import { PlayerBar } from "@/components/PlayerBar"
import { PreviewProvider } from "@/components/PreviewCard"
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar"
import { useIsMobile } from "@/hooks/use-mobile"
import { usePlaybackEventsBridge } from "@/lib/playback-events"
import { usePlaybackKeys } from "@/lib/playback-keys"
import { usePlayerState } from "@/lib/playback-queries"
import { useSettings } from "@/lib/settings-queries"
import { READER_ROUTE } from "@/reader/route"
import { RouteScrollContext } from "@/lib/route-scroll"
import { sidebarShouldBeOpen, sidebarShouldOverlayContent } from "@/lib/sidebar-state"
import { useScrollRestoration } from "@/lib/scroll-restoration"
import mediaFlickLogo from "../../../distribution/app-icon.svg"

const PLAYER_CHROME_HIDE_DELAY_MS = 3000
const PLAYER_CHROME_REVEAL_DEADZONE_PX = 10

interface PointerPosition {
  clientX: number
  clientY: number
}

function pointerLeftDeadzone(origin: PointerPosition, event: MouseEvent) {
  const deltaX = event.clientX - origin.clientX
  const deltaY = event.clientY - origin.clientY
  return deltaX * deltaX + deltaY * deltaY > PLAYER_CHROME_REVEAL_DEADZONE_PX ** 2
}

export function RouteScrollViewport({ children }: { children: ReactNode }) {
  const [viewport, setViewport] = useState<HTMLDivElement | null>(null)
  useScrollRestoration(viewport, "route")

  return (
    <div ref={setViewport} className="content-viewport min-h-0 min-w-0 max-w-full flex-1 overflow-x-hidden overflow-y-auto">
      <RouteScrollContext.Provider value={viewport}>{children}</RouteScrollContext.Provider>
    </div>
  )
}

/**
 * The Built-in player's chrome (brand and player bar) and the cursor show
 * together when the pointer moves past a deadzone anywhere over the window,
 * and hide after a few seconds without such a move. They stay while paused,
 * while a bar menu is open, and while the pointer or keyboard focus is in the
 * bar.
 */
function IntegratedPlayerOverlay({ paused }: { paused: boolean }) {
  const [pointerIdle, setPointerIdle] = useState(false)
  const [playerMenuOpen, setPlayerMenuOpen] = useState(false)
  const [pointerOverBar, setPointerOverBar] = useState(false)
  const [keyboardFocusInBar, setKeyboardFocusInBar] = useState(false)
  const held = paused || playerMenuOpen || pointerOverBar || keyboardFocusInBar
  // A hold resets idleness, so the chrome stays for the full delay after it.
  if (held && pointerIdle) setPointerIdle(false)
  const chromeVisible = held || !pointerIdle

  // `origin` is where the pointer last counted as moving, or where it rested
  // when the chrome hid; null until the first move, which only sets it.
  const pointer = useRef<{ origin: PointerPosition | null; last: PointerPosition | null }>({ origin: null, last: null })

  useEffect(() => {
    let hideTimer: number | undefined
    const scheduleHide = () => {
      window.clearTimeout(hideTimer)
      if (held) return
      hideTimer = window.setTimeout(() => {
        pointer.current.origin = pointer.current.last
        setPointerIdle(true)
      }, PLAYER_CHROME_HIDE_DELAY_MS)
    }
    const handleMouseMove = (event: MouseEvent) => {
      const { origin } = pointer.current
      pointer.current.last = { clientX: event.clientX, clientY: event.clientY }
      if (origin !== null && !pointerLeftDeadzone(origin, event)) return
      pointer.current.origin = pointer.current.last
      if (origin === null) return
      setPointerIdle(false)
      scheduleHide()
    }
    const handleVisibilityChange = () => {
      if (!document.hidden) scheduleHide()
    }

    scheduleHide()

    window.addEventListener("mousemove", handleMouseMove)
    window.addEventListener("resize", scheduleHide)
    window.addEventListener("focus", scheduleHide)
    window.addEventListener("pageshow", scheduleHide)
    document.addEventListener("fullscreenchange", scheduleHide)
    document.addEventListener("visibilitychange", handleVisibilityChange)
    return () => {
      window.clearTimeout(hideTimer)
      window.removeEventListener("mousemove", handleMouseMove)
      window.removeEventListener("resize", scheduleHide)
      window.removeEventListener("focus", scheduleHide)
      window.removeEventListener("pageshow", scheduleHide)
      document.removeEventListener("fullscreenchange", scheduleHide)
      document.removeEventListener("visibilitychange", handleVisibilityChange)
    }
  }, [held])

  useLayoutEffect(() => {
    document.documentElement.toggleAttribute("data-libmpv-cursor-hidden", !chromeVisible)
    return () => document.documentElement.removeAttribute("data-libmpv-cursor-hidden")
  }, [chromeVisible])

  return (
    <main className="libmpv-overlay pointer-events-none flex h-full min-w-0 flex-col justify-between overflow-hidden">
      <div
        className="libmpv-overlay-brand libmpv-overlay-chrome"
        data-visible={chromeVisible}
        aria-label="MediaFlick"
        aria-hidden={!chromeVisible}
      >
        <img src={mediaFlickLogo} alt="" />
        <span>
          Media<span>Flick</span>
        </span>
      </div>
      <div
        className="libmpv-overlay-chrome pointer-events-auto"
        data-visible={chromeVisible}
        aria-hidden={!chromeVisible}
        inert={!chromeVisible}
        onPointerEnter={() => setPointerOverBar(true)}
        onPointerLeave={() => setPointerOverBar(false)}
        onFocus={(event) => {
          // Focus inside a menu's portal bubbles here too; the open menu holds the bar then.
          setKeyboardFocusInBar(event.currentTarget.contains(event.target) && event.target.matches(":focus-visible"))
        }}
        onBlur={(event) => {
          if (!(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))) {
            setKeyboardFocusInBar(false)
          }
        }}
      >
        <PlayerBar onMenuOpenChange={setPlayerMenuOpen} />
      </div>
    </main>
  )
}

export function AppShell({ children }: { children: ReactNode }) {
  const location = useLocation()
  const isMobile = useIsMobile()
  const [pointerIsOverSidebar, setPointerIsOverSidebar] = useState(false)
  const [userMenuIsOpen, setUserMenuIsOpen] = useState(false)
  const sidebarOpen = isMobile || sidebarShouldBeOpen(location.pathname, pointerIsOverSidebar, userMenuIsOpen)
  const settings = useSettings()
  const player = usePlayerState()
  const viewing = useViewing()
  const cardPreviews = settings.data?.appearance.cardPreviews !== false
  const integratedPlayback = Boolean(
    settings.data?.capabilities.integratedLibmpvOverlay &&
      player.data?.active &&
      player.data.nowPlaying?.kind === "video",
  )
  usePlaybackEventsBridge()
  usePlaybackKeys()

  useLayoutEffect(() => {
    document.documentElement.toggleAttribute("data-libmpv-playback", integratedPlayback)
    return () => document.documentElement.removeAttribute("data-libmpv-playback")
  }, [integratedPlayback])

  if (integratedPlayback) {
    return <IntegratedPlayerOverlay paused={Boolean(player.data?.paused)} />
  }

  if (matchPath(READER_ROUTE, location.pathname)) {
    return <main className="h-full min-h-0 bg-background">{children}</main>
  }

  return (
    <SidebarProvider
      open={sidebarOpen}
      data-sidebar-overlay={sidebarShouldOverlayContent(location.pathname) || undefined}
      className="app-experience h-full min-w-0 overflow-hidden"
    >
      <AppSidebar
        onPointerEnter={(event) => {
          // React also bubbles enter from the portalled user menu, whose
          // unmount under the pointer never sends the matching leave.
          if (event.target instanceof Node && event.currentTarget.contains(event.target)) {
            setPointerIsOverSidebar(true)
          }
        }}
        onPointerLeave={() => setPointerIsOverSidebar(false)}
        onUserMenuOpenChange={setUserMenuIsOpen}
      />
      <SidebarInset className="isolate min-h-0 min-w-0 overflow-hidden">
        <RouteScrollViewport>
          <PreviewProvider enabled={cardPreviews} delay={viewing.data?.previewDelayMs}>{children}</PreviewProvider>
        </RouteScrollViewport>
        <PlayerBar />
      </SidebarInset>
    </SidebarProvider>
  )
}
