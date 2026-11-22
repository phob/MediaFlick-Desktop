import { ViewingContext, useViewing } from "@/lib/viewing"
import { ViewingSync } from "@/components/ViewingSync"
import { Navigate, Route, Routes, useLocation } from "react-router-dom"
import { AppShell } from "@/components/AppShell"
import { BootstrapRetry } from "@/components/BootstrapRetry"
import { LoadingScreen } from "@/components/LoadingScreen"
import { StartupFailure } from "@/components/StartupFailure"
import { SeerrGate } from "@/components/seerr/SeerrGate"
import { WhatsNewDialog } from "@/components/WhatsNewDialog"
import { WindowTitleSync } from "@/components/WindowTitleSync"
import { ApiError } from "@/lib/api"
import { RatingsProvider } from "@/lib/ratings"
import { TechnicalProvider } from "@/lib/technical"
import { useLibraryMetadataBridge } from "@/lib/library-events"
import { useBillboard, useHome } from "@/lib/home-queries"
import { useStatusPulse } from "@/lib/session-queries"
import { useSettings } from "@/lib/settings-queries"
import { startupScreenReady } from "@/lib/startup"
import Discover from "@/routes/Discover"
import DiscoverDetail from "@/routes/DiscoverDetail"
import Collections, {
  CollectionModeRoute,
  FranchiseCollections,
  JellyfinCollections,
  MyCollections,
} from "@/routes/Collections"
import {
  FranchiseCollectionDetail,
  JellyfinCollectionDetail,
  MyCollectionDetail,
} from "@/routes/CollectionDetail"
import Home from "@/routes/Home"
import ItemDetail from "@/routes/ItemDetail"
import Reader from "@/reader/Reader"
import { READER_ROUTE } from "@/reader/route"
import Library from "@/routes/Library"
import Requests from "@/routes/Requests"
import Calendar from "@/routes/Calendar"
import SignIn from "@/routes/SignIn"
import { AppearanceSync } from "@/components/AppearanceSync"
import Settings from "@/routes/Settings"

export default function App() {
  const { data: status, isPending, error } = useStatusPulse()
  const startupFailure = error instanceof ApiError && error.startupFailed ? error.message : null
  const settings = useSettings()
  const viewing = useViewing()
  const location = useLocation()
  useLibraryMetadataBridge()
  const bootstrap = status?.authenticated ? status.bootstrap : null
  const catalogLoading = bootstrap?.state === "loading"
  const showingSettings = location.pathname === "/settings" || location.pathname.startsWith("/settings/")
  const showingHome = location.pathname === "/"
  const showShell = Boolean(status?.authenticated || showingSettings)
  const initialHomeEnabled = bootstrap?.state === "ready" && showingHome
  const initialHome = useHome(initialHomeEnabled)
  const initialBillboard = useBillboard(initialHomeEnabled)
  const ready = startupScreenReady({
    statusPending: isPending,
    settingsPending: settings.isPending,
    catalogLoading,
    showingSettings,
    initialHomeEnabled,
    homePending: initialHome.isPending,
    billboardPending: initialBillboard.isPending,
  })

  return (
    <ViewingContext value={viewing.data ?? null}>
      {startupFailure !== null ? (
        <StartupFailure reason={startupFailure} />
      ) : isPending || (catalogLoading && !showingSettings) ? (
        <div className="h-full bg-background" aria-hidden />
      ) : bootstrap?.state === "retrying" && !showingSettings ? (
        <BootstrapRetry retry={bootstrap} attempting={Boolean(status?.syncing)} />
      ) : !showShell ? (
        <SignIn />
      ) : (
        <RatingsProvider>
          <TechnicalProvider>
            <AppShell>
            <Routes>
              <Route path="/settings/*" element={<Settings />} />
              <Route path="/" element={<Home />} />
              <Route path="/library" element={<Library />} />
              <Route path="/library/:libraryId" element={<Library />} />
              <Route path="/collections" element={<Collections />} />
              <Route element={<CollectionModeRoute mode="mediaFlick" />}>
                <Route path="/collections/franchises" element={<FranchiseCollections />} />
                <Route path="/collections/franchises/:tmdbCollectionId" element={<FranchiseCollectionDetail />} />
                <Route path="/collections/mine" element={<MyCollections />} />
                <Route path="/collections/mine/:profileId" element={<MyCollectionDetail />} />
              </Route>
              <Route element={<CollectionModeRoute mode="jellyfin" />}>
                <Route path="/collections/jellyfin" element={<JellyfinCollections />} />
                <Route path="/collections/jellyfin/:boxSetId" element={<JellyfinCollectionDetail />} />
              </Route>
              <Route path="/item/:id" element={<ItemDetail />} />
              <Route path={READER_ROUTE} element={<Reader />} />
              <Route path="/calendar" element={<Calendar />} />
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
                element={<DiscoverDetail />}
              />
              <Route
                path="/requests"
                element={
                  <SeerrGate>
                    <Requests />
                  </SeerrGate>
                }
              />
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
            </AppShell>
            <WhatsNewDialog />
          </TechnicalProvider>
        </RatingsProvider>
      )}
      <LoadingScreen
        key={status?.authenticated ? "authenticated" : "anonymous"}
        ready={ready}
      />
      <WindowTitleSync />
      <AppearanceSync />
      <ViewingSync />
    </ViewingContext>
  )
}
