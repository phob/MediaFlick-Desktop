import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react"
import { useLocation, useNavigationType } from "react-router-dom"
import { AppSidebar } from "@/components/AppSidebar"
import { PlayerBar } from "@/components/PlayerBar"
import { PreviewProvider } from "@/components/PreviewCard"
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar"
import { useIsMobile } from "@/hooks/use-mobile"
import { usePlaybackEventsBridge } from "@/lib/playback-events"
import { useLibraryMetadataBridge } from "@/lib/library-events"
import { usePlayerState, useSettings } from "@/lib/queries"
import { sidebarShouldBeOpen, sidebarShouldOverlayContent } from "@/lib/sidebar-state"
import mediaFlickLogo from "../../../resources/app-icon.svg"

const routeScrollPositions = new Map<string, number>()
const PLAYER_CHROME_HIDE_DELAY_MS = 3000
const CURSOR_REVEAL_DISTANCE_PX = 10
const PLAYER_BAR_REVEAL_HEIGHT_PX = 112
const PLAYER_BRAND_REVEAL_WIDTH_PX = 220
const PLAYER_BRAND_REVEAL_HEIGHT_PX = 90

function pointerIsNearPlayerChrome(event: MouseEvent) {
  const nearPlayerBar = event.clientY >= window.innerHeight - PLAYER_BAR_REVEAL_HEIGHT_PX
  const nearBrand =
    event.clientX <= PLAYER_BRAND_REVEAL_WIDTH_PX &&
    event.clientY <= PLAYER_BRAND_REVEAL_HEIGHT_PX
  return nearPlayerBar || nearBrand
}

interface PointerPosition {
  clientX: number
  clientY: number
}

function pointerMovedFarEnough(event: MouseEvent, origin: PointerPosition) {
  const deltaX = event.clientX - origin.clientX
  const deltaY = event.clientY - origin.clientY
  return deltaX * deltaX + deltaY * deltaY >= CURSOR_REVEAL_DISTANCE_PX ** 2
}

/** The route-owned scroll container, separated from sidebar/player chrome for focused testing. */
export function RouteScrollViewport({ children }: { children: ReactNode }) {
  const viewport = useRef<HTMLDivElement>(null)
  const location = useLocation()
  const navigationType = useNavigationType()

  useLayoutEffect(() => {
    const element = viewport.current
    if (!element) return
    const top = navigationType === "POP" ? routeScrollPositions.get(location.key) ?? 0 : 0
    element.scrollTo({ top })
    return () => {
      routeScrollPositions.set(location.key, element.scrollTop)
    }
  }, [location.key, navigationType])

  return (
    <div ref={viewport} className="content-viewport min-h-0 min-w-0 max-w-full flex-1 overflow-x-hidden overflow-y-auto">
      {children}
    </div>
  )
}

function IntegratedPlayerOverlay({ paused }: { paused: boolean }) {
  const [playerChromeVisible, setPlayerChromeVisible] = useState(true)
  const [cursorRecentlyMoved, setCursorRecentlyMoved] = useState(true)
  const [playerMenuOpen, setPlayerMenuOpen] = useState(false)
  const chromeVisible = paused || playerMenuOpen || playerChromeVisible
  const cursorVisible = paused || playerMenuOpen || cursorRecentlyMoved
  const handlePlayerMenuOpenChange = (open: boolean) => {
    setPlayerMenuOpen(open)
    if (open) setPlayerChromeVisible(true)
  }

  useEffect(() => {
    let chromeHideTimer: number | undefined
    let cursorHideTimer: number | undefined
    let pointerPosition: PointerPosition | undefined
    let cursorRevealOrigin: PointerPosition | null | undefined
    const clearHideTimers = () => {
      if (chromeHideTimer !== undefined) window.clearTimeout(chromeHideTimer)
      if (cursorHideTimer !== undefined) window.clearTimeout(cursorHideTimer)
      chromeHideTimer = undefined
      cursorHideTimer = undefined
    }
    const scheduleChromeHide = () => {
      if (chromeHideTimer !== undefined) window.clearTimeout(chromeHideTimer)
      if (paused || playerMenuOpen) return
      chromeHideTimer = window.setTimeout(
        () => setPlayerChromeVisible(false),
        PLAYER_CHROME_HIDE_DELAY_MS,
      )
    }
    const scheduleCursorHide = () => {
      if (cursorHideTimer !== undefined) window.clearTimeout(cursorHideTimer)
      if (paused || playerMenuOpen) return
      cursorHideTimer = window.setTimeout(() => {
        cursorRevealOrigin = pointerPosition ?? null
        setCursorRecentlyMoved(false)
      }, PLAYER_CHROME_HIDE_DELAY_MS)
    }
    const handleMouseMove = (event: MouseEvent) => {
      const previousPosition = pointerPosition
      pointerPosition = { clientX: event.clientX, clientY: event.clientY }

      if (cursorRevealOrigin !== undefined) {
        if (cursorRevealOrigin === null) {
          cursorRevealOrigin = pointerPosition
          return
        }
        if (!pointerMovedFarEnough(event, cursorRevealOrigin)) return
        cursorRevealOrigin = undefined
      } else if (
        previousPosition === undefined ||
        (event.clientX === previousPosition.clientX && event.clientY === previousPosition.clientY)
      ) {
        return
      }

      setCursorRecentlyMoved(true)
      scheduleCursorHide()
      if (!pointerIsNearPlayerChrome(event)) return
      setPlayerChromeVisible(true)
      scheduleChromeHide()
    }
    const rearmAutoHide = () => {
      scheduleChromeHide()
      scheduleCursorHide()
    }
    const handleVisibilityChange = () => {
      if (!document.hidden) rearmAutoHide()
    }

    if (!paused && !playerMenuOpen) rearmAutoHide()

    window.addEventListener("mousemove", handleMouseMove)
    window.addEventListener("resize", rearmAutoHide)
    window.addEventListener("focus", rearmAutoHide)
    window.addEventListener("pageshow", rearmAutoHide)
    document.addEventListener("fullscreenchange", rearmAutoHide)
    document.addEventListener("visibilitychange", handleVisibilityChange)
    return () => {
      clearHideTimers()
      window.removeEventListener("mousemove", handleMouseMove)
      window.removeEventListener("resize", rearmAutoHide)
      window.removeEventListener("focus", rearmAutoHide)
      window.removeEventListener("pageshow", rearmAutoHide)
      document.removeEventListener("fullscreenchange", rearmAutoHide)
      document.removeEventListener("visibilitychange", handleVisibilityChange)
    }
  }, [paused, playerMenuOpen])

  useLayoutEffect(() => {
    document.documentElement.toggleAttribute("data-libmpv-cursor-hidden", !cursorVisible)
    return () => document.documentElement.removeAttribute("data-libmpv-cursor-hidden")
  }, [cursorVisible])

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
      >
        <PlayerBar onMenuOpenChange={handlePlayerMenuOpenChange} />
      </div>
    </main>
  )
}

export function AppShell({ children }: { children: ReactNode }) {
  const location = useLocation()
  const isMobile = useIsMobile()
  const [pointerIsOverSidebar, setPointerIsOverSidebar] = useState(false)
  const sidebarOpen = isMobile || sidebarShouldBeOpen(location.pathname, pointerIsOverSidebar)
  const settings = useSettings()
  const player = usePlayerState()
  const cardPreviews = settings.data?.appearance.cardPreviews !== false
  const integratedPlayback = Boolean(
    settings.data?.capabilities.integratedLibmpvOverlay &&
      player.data?.active,
  )
  usePlaybackEventsBridge()
  useLibraryMetadataBridge()

  useLayoutEffect(() => {
    document.documentElement.toggleAttribute("data-libmpv-playback", integratedPlayback)
    return () => document.documentElement.removeAttribute("data-libmpv-playback")
  }, [integratedPlayback])

  if (integratedPlayback) {
    return <IntegratedPlayerOverlay paused={Boolean(player.data?.paused)} />
  }

  return (
    <SidebarProvider
      open={sidebarOpen}
      onOpenChange={() => undefined}
      data-sidebar-overlay={sidebarShouldOverlayContent(location.pathname) || undefined}
      className="app-experience h-full min-w-0 overflow-hidden"
    >
      <AppSidebar
        onPointerEnter={() => setPointerIsOverSidebar(true)}
        onPointerLeave={() => setPointerIsOverSidebar(false)}
      />
      <SidebarInset className="isolate min-h-0 min-w-0 overflow-hidden">
        {/* The shell never scrolls; the content pane does. */}
        <RouteScrollViewport>
          {/* Wraps the routed content because every card that can expand is in
              it. The panel itself is portalled to the body, so this subtree's
              clipping — `content-viewport` and the rails — does not reach it. */}
          <PreviewProvider enabled={cardPreviews}>{children}</PreviewProvider>
        </RouteScrollViewport>
        <PlayerBar />
      </SidebarInset>
    </SidebarProvider>
  )
}
