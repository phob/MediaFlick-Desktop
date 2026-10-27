import {
  AlertTriangle,
  ChevronsUpDown,
  CalendarDays,
  Compass,
  Film,
  Heart,
  House,
  Inbox,
  Layers,
  LogOut,
  RefreshCw,
  Search,
  Settings,
} from "lucide-react"
import { useQueryClient } from "@tanstack/react-query"
import { useCallback, useEffect, type ComponentProps } from "react"
import { Link, useLocation, useNavigate } from "react-router-dom"
import { toast } from "sonner"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInput,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
} from "@/components/ui/sidebar"
import { useSidebar } from "@/components/ui/sidebar-context"
import { useLiveSearch } from "@/hooks/use-live-search"
import { useCompanionCapabilities } from "@/lib/companion"
import { api } from "@/lib/api"
import { toastFailure } from "@/lib/failure-toast"
import { libraryIcon, libraryIdFromPath, libraryPath } from "@/lib/libraries"
import { useLocalDate } from "@/lib/local-date"
import { isSidebarRouteActive, librarySearchFromLocation, readDetailNavigationState } from "@/lib/navigation"
import {
  accountKey,
  franchisesQueryOptions,
  myCollectionsQueryOptions,
  useCollectionSettings,
  useItem,
  useLibraries,
  useLogout,
  useStatus,
  useSyncProgress,
} from "@/lib/queries"
import { SYNC_FAILURES } from "@/lib/sync-failure"

const NAV = [
  { title: "Home", to: "/", icon: House },
  { title: "Favorites", to: "/library?favorite=true", icon: Heart },
]

const SEERR_NAV = [
  { title: "Discover", to: "/discover", icon: Compass },
  { title: "Requests", to: "/requests", icon: Inbox },
]

function serverLabel(url: string | null | undefined) {
  if (!url) return "Not connected"
  try {
    return new URL(url).host
  } catch {
    return url
  }
}

export function SearchBox() {
  const { state } = useSidebar()
  const location = useLocation()
  const navigate = useNavigate()
  const locationSearch = librarySearchFromLocation(location.pathname, location.search)
  const commitSearch = useCallback(
    (term: string) => {
      if (term) navigate(`/library?search=${encodeURIComponent(term)}`, { replace: true })
      else if (locationSearch) navigate("/library", { replace: true })
    },
    [locationSearch, navigate],
  )
  const [search, setSearch, flushSearch] = useLiveSearch(locationSearch, commitSearch)

  if (state === "collapsed") {
    return (
      <SidebarMenu>
        <SidebarMenuItem>
          <SidebarMenuButton asChild tooltip="Search">
            <Link to="/library">
              <Search />
              <span>Search</span>
            </Link>
          </SidebarMenuButton>
        </SidebarMenuItem>
      </SidebarMenu>
    )
  }

  return (
    <form
      className="relative"
      onSubmit={(event) => {
        event.preventDefault()
        flushSearch()
      }}
    >
      <Search className="pointer-events-none absolute top-1/2 left-2 size-4 -translate-y-1/2 text-muted-foreground" />
      <SidebarInput
        value={search}
        onChange={(event) => setSearch(event.target.value)}
        placeholder="Search…"
        aria-label="Search the library"
        minLength={2}
        className="pl-8"
      />
    </form>
  )
}

export function LibrarySyncProgress() {
  const { data: progress } = useSyncProgress()
  if (!progress) return null

  const catalog = progress.catalog
  const retry = progress.retry
  const isRetrying = retry !== null
  const phase = isRetrying
    ? "Synchronization paused"
    : progress.phase === "catalog"
      ? "Loading library"
      : "Refreshing library"
  const current = catalog.processed
  const total = catalog.total
  const determinate = !isRetrying && progress.phase === "catalog"
  const error = retry ? SYNC_FAILURES[retry.reason].title : null
  const detail = retry
    ? `Retry scheduled for ${new Date(retry.retryAt * 1_000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
    : determinate && total != null && total > 0
      ? `${Math.min(current, total).toLocaleString()} of ${total.toLocaleString()}`
      : "Working in the background"

  return (
    <div
      className="sidebar-sync-progress"
      data-error={isRetrying || undefined}
      role="status"
      tabIndex={0}
      aria-live="polite"
      aria-atomic="true"
      aria-label={[phase, detail, error].filter(Boolean).join(". ")}
      title={error ?? `${phase}: ${detail}`}
    >
      {isRetrying ? <AlertTriangle aria-hidden /> : <RefreshCw aria-hidden />}
      <div className="sidebar-sync-copy">
        <strong>{phase}</strong>
        <span>{detail}</span>
        <div
          className="sidebar-sync-track"
          role="progressbar"
          aria-label={phase}
          aria-valuemin={determinate ? 0 : undefined}
          aria-valuemax={determinate && total != null ? total : undefined}
          aria-valuenow={determinate && total != null ? Math.min(current, total) : undefined}
          aria-valuetext={detail}
        >
          <span
            style={
              determinate && total != null && total > 0
                ? { width: `${Math.min(100, (current / total) * 100)}%` }
                : undefined
            }
          />
        </div>
        {error && <span className="sr-only">{error}</span>}
      </div>
    </div>
  )
}

// The open menu holds the hover sidebar expanded: as a modal menu it disables
// pointer events outside itself, so the sidebar sees the pointer leave.
function UserMenu({ onOpenChange }: { onOpenChange?: (open: boolean) => void }) {
  const { data: status } = useStatus()
  const logout = useLogout()
  const name = status?.userName ?? "Signed in"

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu onOpenChange={onOpenChange}>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              tooltip={name}
              className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
            >
              <div className="flex aspect-square size-8 shrink-0 items-center justify-center rounded-full bg-sidebar-accent text-sm font-medium uppercase">
                {name.slice(0, 1)}
              </div>
              <div className="grid flex-1 text-left leading-tight">
                <span className="truncate font-medium">{name}</span>
                <span className="truncate text-xs text-muted-foreground">
                  {serverLabel(status?.serverUrl)}
                </span>
              </div>
              <ChevronsUpDown className="ml-auto size-4" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent side="right" align="end" sideOffset={8} className="w-56">
            <DropdownMenuLabel className="font-normal">
              <div className="grid leading-tight">
                <span className="truncate font-medium">{name}</span>
                <span className="truncate text-xs text-muted-foreground">
                  {serverLabel(status?.serverUrl)}
                </span>
              </div>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              disabled={status?.syncing}
              onSelect={() => {
                void api
                  .sync()
                  .then(() => toast.success("Sync requested"))
                  .catch((error: Error) => toastFailure(error))
              }}
            >
              <RefreshCw />
              {status?.syncing ? "Syncing…" : "Sync library"}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => logout.mutate()}>
              <LogOut />
              Sign out
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}

type AppSidebarProps = Pick<ComponentProps<typeof Sidebar>, "onPointerEnter" | "onPointerLeave"> & {
  onUserMenuOpenChange?: (open: boolean) => void
}

export function AppSidebar({ onUserMenuOpenChange, ...props }: AppSidebarProps = {}) {
  const location = useLocation()
  const cache = useQueryClient()
  const { data: status } = useStatus()
  const account = accountKey(status)
  const localDate = useLocalDate()
  const libraryParams = new URLSearchParams(location.search)
  const detailMatch = location.pathname.match(/^\/item\/([^/]+)$/)
  let detailId: string | undefined
  try {
    detailId = detailMatch ? decodeURIComponent(detailMatch[1]) : undefined
  } catch {
    detailId = undefined
  }
  const detail = useItem(detailId)
  const libraries = useLibraries()
  const detailOrigin = readDetailNavigationState(location.state)
  const originUrl = detailOrigin?.from.startsWith("/library")
    ? new URL(detailOrigin.from, "https://mediaflick.invalid")
    : null
  const activeLibraryId = detailId === undefined
    ? libraryIdFromPath(location.pathname)
    : originUrl
      ? libraryIdFromPath(originUrl.pathname)
      : detail.data?.layout === "title"
        ? detail.data.libraryId
        : null
  const favoritesActive = location.pathname === "/library"
    ? libraryParams.get("favorite") === "true" && !libraryParams.has("kind")
    : detailId !== undefined
      && originUrl?.pathname === "/library"
      && originUrl.searchParams.get("favorite") === "true"
      && !originUrl.searchParams.has("kind")
  const companionSeerr = useCompanionCapabilities().has("seerr")
  const { data: collectionSettings } = useCollectionSettings()
  const collectionNavigation = collectionSettings?.effectiveMode === "mediaFlick"
    ? [
        { title: "Movie Franchises", to: "/collections/franchises" },
        { title: "My Collections", to: "/collections/mine" },
      ]
    : [{ title: "Jellyfin Collections", to: "/collections/jellyfin" }]
  const prefetchCollection = useCallback((path: string) => {
    if (!status?.authenticated) return
    if (path === "/collections/franchises") {
      void cache.prefetchQuery(franchisesQueryOptions(account, localDate))
    } else if (path === "/collections/mine") {
      void cache.prefetchQuery(myCollectionsQueryOptions(account))
    }
  }, [account, cache, localDate, status?.authenticated])

  useEffect(() => {
    if (!status?.authenticated || collectionSettings?.effectiveMode !== "mediaFlick") return
    const warm = () => {
      prefetchCollection("/collections/franchises")
      prefetchCollection("/collections/mine")
    }
    if (window.requestIdleCallback) {
      const idle = window.requestIdleCallback(warm, { timeout: 2_000 })
      return () => window.cancelIdleCallback(idle)
    }
    const timer = window.setTimeout(warm, 750)
    return () => window.clearTimeout(timer)
  }, [collectionSettings?.effectiveMode, prefetchCollection, status?.authenticated])

  return (
    <Sidebar className="app-sidebar-container" {...props}>
      <SidebarHeader>
        <div className="flex items-center gap-2">
          <Link
            to="/"
            className="flex min-w-0 items-center gap-2 font-medium"
          >
            <div className="flex aspect-square size-8 shrink-0 items-center justify-center rounded-media bg-primary text-primary-foreground shadow-lg shadow-primary/25">
              <Film className="size-4" />
            </div>
            <span className="truncate text-[0.95rem] font-semibold tracking-tight group-data-[collapsible=icon]:hidden">
              Media<span className="text-primary">Flick</span>
            </span>
          </Link>
        </div>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup className="py-0">
          <SearchBox />
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Library</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {NAV.map((item) => (
                <SidebarMenuItem key={item.to}>
                  <SidebarMenuButton
                    asChild
                    isActive={item.title === "Home" ? location.pathname === "/" : favoritesActive}
                    tooltip={item.title}
                  >
                    <Link to={item.to}>
                      <item.icon />
                      <span>{item.title}</span>
                    </Link>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
              <SidebarMenuItem>
                <SidebarMenuButton
                  asChild
                  isActive={location.pathname === "/calendar"}
                  tooltip="Releases"
                >
                  <Link to="/calendar">
                    <CalendarDays />
                    <span>Releases</span>
                  </Link>
                </SidebarMenuButton>
              </SidebarMenuItem>
              {collectionNavigation.map((item) => (
                <SidebarMenuItem key={item.to}>
                  <SidebarMenuButton
                    asChild
                    isActive={location.pathname.startsWith(item.to)}
                    tooltip={item.title}
                  >
                    <Link
                      to={item.to}
                      onPointerEnter={() => prefetchCollection(item.to)}
                      onFocus={() => prefetchCollection(item.to)}
                    >
                      <Layers />
                      <span>{item.title}</span>
                    </Link>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Libraries</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {libraries.data?.known
                ? libraries.data.libraries.map((library) => {
                    const Icon = libraryIcon(library.kind)
                    return (
                      <SidebarMenuItem key={library.id}>
                        <SidebarMenuButton
                          asChild
                          isActive={activeLibraryId === library.id}
                          tooltip={library.name}
                        >
                          <Link to={libraryPath(library.id)}>
                            <Icon />
                            <span>{library.name}</span>
                          </Link>
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    )
                  })
                : !libraries.isError && Array.from({ length: 2 }, (_, index) => (
                    <SidebarMenuItem key={index}>
                      <SidebarMenuSkeleton showIcon />
                    </SidebarMenuItem>
                  ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        {companionSeerr && (
          <SidebarGroup>
            <SidebarGroupLabel>Seerr</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {SEERR_NAV.map((item) => (
                  <SidebarMenuItem key={item.to}>
                    <SidebarMenuButton
                      asChild
                      isActive={isSidebarRouteActive(item.to, location.pathname)}
                      tooltip={item.title}
                    >
                      <Link to={item.to}>
                        <item.icon />
                        <span>{item.title}</span>
                      </Link>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        )}
      </SidebarContent>

      <SidebarFooter>
        <LibrarySyncProgress />
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild isActive={location.pathname.startsWith("/settings")} tooltip="Settings">
              <Link to="/settings">
                <Settings />
                <span>Settings</span>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
        <UserMenu onOpenChange={onUserMenuOpenChange} />
      </SidebarFooter>
    </Sidebar>
  )
}
