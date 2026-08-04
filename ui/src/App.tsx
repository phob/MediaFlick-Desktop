import { Route, Routes, useLocation } from "react-router-dom"
import { AppShell } from "@/components/AppShell"
import { LoadingScreen } from "@/components/LoadingScreen"
import { SeerrGate } from "@/components/seerr/SeerrGate"
import { useStatus } from "@/lib/queries"
import Discover from "@/routes/Discover"
import DiscoverDetail from "@/routes/DiscoverDetail"
import Home from "@/routes/Home"
import ItemDetail from "@/routes/ItemDetail"
import Library from "@/routes/Library"
import Requests from "@/routes/Requests"
import Calendar from "@/routes/Calendar"
import SignIn from "@/routes/SignIn"
import Settings, { AppearanceSync } from "@/routes/Settings"

export default function App() {
  const { data: status, isPending } = useStatus()
  const location = useLocation()
  const waitingForLibrary = Boolean(
    status?.authenticated && !(status.libraryReady ?? status.bootstrapped),
  )
  // Player and appearance configuration has always been available from the
  // sign-in screen. Keep that promise while the rest of the app remains
  // account-gated: a direct /settings link is a real anonymous route.
  const showingSettings = location.pathname === "/settings" || location.pathname.startsWith("/settings/")
  const showShell = Boolean(status?.authenticated || showingSettings)

  return (
    <>
      {isPending || (waitingForLibrary && !showingSettings) ? (
        <div className="h-full bg-background" aria-hidden />
      ) : !showShell ? (
        // One gate for the whole app: the Rust session is the source of truth,
        // and `/api/status` re-reports it after the server rejects a stored
        // token.
        <SignIn />
      ) : (
        <AppShell>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
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
              path="/discover/:mediaType/:tmdbId"
              element={
                <SeerrGate>
                  <DiscoverDetail />
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
            <Route path="*" element={status?.authenticated ? <Home /> : <SignIn />} />
          </Routes>
        </AppShell>
      )}
      <LoadingScreen
        // A new account waits only for the first committed catalog page. The
        // rest of the fill and all rich enrichment happen in the live shell.
        key={status?.authenticated ? "authenticated" : "anonymous"}
        ready={!isPending && (!waitingForLibrary || showingSettings)}
      />
      <AppearanceSync />
    </>
  )
}
