import { Film } from "lucide-react"
import { Route, Routes } from "react-router-dom"
import { AppShell } from "@/components/AppShell"
import { SeerrGate } from "@/components/seerr/SeerrGate"
import { useStatus } from "@/lib/queries"
import Discover from "@/routes/Discover"
import Home from "@/routes/Home"
import ItemDetail from "@/routes/ItemDetail"
import Library from "@/routes/Library"
import Requests from "@/routes/Requests"
import SignIn from "@/routes/SignIn"

export default function App() {
  const { data: status, isPending } = useStatus()

  if (isPending) {
    return (
      <div className="signin-page grid h-full place-items-center">
        <div className="flex flex-col items-center gap-3">
          <div className="grid size-12 animate-pulse place-items-center rounded-xl bg-primary text-white">
            <Film className="size-6" />
          </div>
          <span className="text-sm font-medium tracking-wide text-muted-foreground">MediaFlick</span>
        </div>
      </div>
    )
  }

  // One gate for the whole app: the Rust session is the source of truth, and
  // `/api/status` re-reports it after the server rejects a stored token.
  if (!status?.authenticated) return <SignIn />

  return (
    <AppShell>
      <Routes>
        <Route path="/" element={<Home />} />
        <Route path="/library" element={<Library />} />
        <Route path="/item/:id" element={<ItemDetail />} />
        {/* Registered whether or not Seerr is linked: the sidebar hides them
            until it is, but a deep link or a session that lapsed mid-use must
            land on the offer to set it up rather than on a blank page. */}
        <Route
          path="/discover"
          element={
            <SeerrGate>
              <Discover />
            </SeerrGate>
          }
        />
        <Route
          path="/requests"
          element={
            <SeerrGate>
              <Requests />
            </SeerrGate>
          }
        />
        <Route path="*" element={<Home />} />
      </Routes>
    </AppShell>
  )
}
