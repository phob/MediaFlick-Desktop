import { useIsFetching } from "@tanstack/react-query"
import { Route, Routes } from "react-router-dom"
import { AppShell } from "@/components/AppShell"
import { LoadingScreen } from "@/components/LoadingScreen"
import { SeerrGate } from "@/components/seerr/SeerrGate"
import { useStatus } from "@/lib/queries"
import Discover from "@/routes/Discover"
import Home from "@/routes/Home"
import ItemDetail from "@/routes/ItemDetail"
import Library from "@/routes/Library"
import Requests from "@/routes/Requests"
import Calendar from "@/routes/Calendar"
import SignIn from "@/routes/SignIn"

export default function App() {
  const { data: status, isPending } = useStatus()
  const fetching = useIsFetching()

  return (
    <>
      {isPending ? (
        <div className="h-full bg-background" aria-hidden />
      ) : !status?.authenticated ? (
        // One gate for the whole app: the Rust session is the source of truth,
        // and `/api/status` re-reports it after the server rejects a stored
        // token.
        <SignIn />
      ) : (
        <AppShell>
          <Routes>
            <Route path="/" element={<Home />} />
            <Route path="/library" element={<Library />} />
            <Route path="/item/:id" element={<ItemDetail />} />
            <Route path="/calendar" element={<Calendar />} />
            {/* Registered whether or not Seerr is linked: the sidebar hides
                them until it is, but a deep link or a session that lapsed
                mid-use must land on the offer to set it up rather than on a
                blank page. */}
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
      )}
      <LoadingScreen ready={!isPending && fetching === 0} />
    </>
  )
}
