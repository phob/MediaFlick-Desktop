import { useLayoutEffect, useRef, useState, type ReactNode } from "react"
import { useLocation, useNavigationType } from "react-router-dom"
import { AppSidebar } from "@/components/AppSidebar"
import { PlayerBar } from "@/components/PlayerBar"
import { PreviewProvider } from "@/components/PreviewCard"
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar"
import { useIsMobile } from "@/hooks/use-mobile"
import { usePlaybackStoppedBridge } from "@/lib/playback-events"
import { useLibraryMetadataBridge } from "@/lib/library-events"
import { useSettings } from "@/lib/queries"
import { sidebarShouldBeOpen, sidebarShouldOverlayContent } from "@/lib/sidebar-state"

const routeScrollPositions = new Map<string, number>()

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

export function AppShell({ children }: { children: ReactNode }) {
  const location = useLocation()
  const isMobile = useIsMobile()
  const [pointerIsOverSidebar, setPointerIsOverSidebar] = useState(false)
  const sidebarOpen = isMobile || sidebarShouldBeOpen(location.pathname, pointerIsOverSidebar)
  const settings = useSettings()
  const cardPreviews = settings.data?.appearance.cardPreviews !== false
  usePlaybackStoppedBridge()
  useLibraryMetadataBridge()

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
