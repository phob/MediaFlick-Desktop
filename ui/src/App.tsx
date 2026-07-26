import { Route, Routes } from "react-router-dom"
import { AppShell } from "@/components/AppShell"
import { Skeleton } from "@/components/ui/skeleton"
import { useStatus } from "@/lib/queries"
import Home from "@/routes/Home"
import ItemDetail from "@/routes/ItemDetail"
import Library from "@/routes/Library"
import SignIn from "@/routes/SignIn"

export default function App() {
  const { data: status, isPending } = useStatus()

  if (isPending) {
    return (
      <div className="grid h-full place-items-center">
        <Skeleton className="h-10 w-48" />
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
        <Route path="*" element={<Home />} />
      </Routes>
      {/* TODO(port): player bar, driven by usePlayerState + /api/player/command. */}
    </AppShell>
  )
}
