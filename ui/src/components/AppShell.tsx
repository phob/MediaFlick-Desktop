import { useState, type ReactNode } from "react"
import { AppSidebar } from "@/components/AppSidebar"
import { PlayerBar } from "@/components/PlayerBar"
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar"
import { usePlaybackStoppedBridge } from "@/lib/playback-events"

const SIDEBAR_OPEN_KEY = "mediaflick.sidebar.open"

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
  usePlaybackStoppedBridge()

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
      className="h-full"
    >
      <AppSidebar />
      <SidebarInset className="min-h-0 overflow-hidden">
        {/* The shell never scrolls; the content pane does. */}
        <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
        <PlayerBar />
      </SidebarInset>
    </SidebarProvider>
  )
}
