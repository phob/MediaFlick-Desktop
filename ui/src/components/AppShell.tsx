import { useLayoutEffect, useRef, useState, type ReactNode } from "react"
import { useLocation, useNavigationType } from "react-router-dom"
import { AppSidebar } from "@/components/AppSidebar"
import { PlayerBar } from "@/components/PlayerBar"
import { PreviewProvider } from "@/components/PreviewCard"
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar"
import { usePlaybackStoppedBridge } from "@/lib/playback-events"
import { useLibraryMetadataBridge } from "@/lib/library-events"

const SIDEBAR_OPEN_KEY = "mediaflick.sidebar.open"
const routeScrollPositions = new Map<string, number>()

function storedSidebarOpen() {
  try {
    return localStorage.getItem(SIDEBAR_OPEN_KEY) !== "false"
  } catch {
    return true
  }
}

export function AppShell({ children }: { children: ReactNode }) {
  // shadcn persists the rail state in a cookie, which the app scheme is not
  // registered for (STANDARD | SECURE | CORS | FETCH — no cookie option), so
  // that write silently no-ops. Drive the provider instead.
  const [open, setOpen] = useState(storedSidebarOpen)
  const viewport = useRef<HTMLDivElement>(null)
  const location = useLocation()
  const navigationType = useNavigationType()
  usePlaybackStoppedBridge()
  useLibraryMetadataBridge()

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
    <SidebarProvider
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        try {
          localStorage.setItem(SIDEBAR_OPEN_KEY, String(next))
        } catch {
          // A blocked storage partition is not a reason to refuse to collapse.
        }
      }}
      className="app-experience h-full min-w-0 overflow-hidden"
    >
      <AppSidebar />
      <SidebarInset className="isolate min-h-0 min-w-0 overflow-hidden">
        {/* The shell never scrolls; the content pane does. */}
        <div ref={viewport} className="content-viewport min-h-0 min-w-0 max-w-full flex-1 overflow-x-hidden overflow-y-auto">
          {/* Wraps the routed content because every card that can expand is in
              it. The panel itself is portalled to the body, so this subtree's
              clipping — `content-viewport` and the rails — does not reach it. */}
          <PreviewProvider>{children}</PreviewProvider>
        </div>
        <PlayerBar />
      </SidebarInset>
    </SidebarProvider>
  )
}
