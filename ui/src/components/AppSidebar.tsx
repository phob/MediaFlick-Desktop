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
  Tv,
} from "lucide-react"
import { useQueryClient } from "@tanstack/react-query"
import { useCallback, useEffect, useRef, type ComponentProps } from "react"
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
} from "@/components/ui/sidebar"
import { useSidebar } from "@/components/ui/sidebar-context"
import { useLiveSearch } from "@/hooks/use-live-search"
import { api } from "@/lib/api"
import { libraryKind, libraryKindPath } from "@/lib/library-filters"
import { useLocalDate } from "@/lib/local-date"
import { isSidebarRouteActive, librarySearchFromLocation, readDetailNavigationState } from "@/lib/navigation"
import {
  collectionAccountKey,
  franchisesQueryOptions,
  myCollectionsQueryOptions,
  useCollectionSettings,
  useCompanion,
  useItem,
  useLogout,
  useStatus,
} from "@/lib/queries"

const NAV = [
  { title: "Home", to: "/", icon: House },
  { title: "Movies", to: libraryKindPath("Movie"), icon: Film },
  { title: "Series", to: libraryKindPath("Series"), icon: Tv },
  { title: "Favorites", to: "/library?favorite=true", icon: Heart },
]

/** Shown only when Companion advertises its typed Seerr capability. */
const SEERR_NAV = [
  { title: "Discover", to: "/discover", icon: Compass },
  { title: "Requests", to: "/requests", icon: Inbox },
]

/** `http://jellyfin.local:8096/` → `jellyfin.local:8096`, unparseable → as-is. */
function serverLabel(url: string | null | undefined) {
  if (!url) return "Not connected"
  try {
    return new URL(url).host
  } catch {
    return url
  }
}

export function SearchBox() {
  const { state, setOpen } = useSidebar()
  const location = useLocation()
  const navigate = useNavigate()
  const input = useRef<HTMLInputElement>(null)
  const locationSearch = librarySearchFromLocation(location.pathname, location.search)
  const commitSearch = useCallback(
    (term: string) => {
      if (term) navigate(`/library?search=${encodeURIComponent(term)}`, { replace: true })
      else if (locationSearch) navigate("/library", { replace: true })
    },
    [locationSearch, navigate],
  )
  // The library URL owns committed searches. The draft can temporarily hold a
  // one-character term while the corresponding unfiltered URL remains active.
  const [search, setSearch, flushSearch] = useLiveSearch(locationSearch, commitSearch)

  // Collapsed to icons there is no room for a field, so the icon stands in for
  // it: expanding and focusing is the same gesture as clicking into it.
  if (state === "collapsed") {
    return (
      <SidebarMenu>
        <SidebarMenuItem>
          <SidebarMenuButton
            tooltip="Search"
            onClick={() => {
              setOpen(true)
              requestAnimationFrame(() => input.current?.focus())
            }}
          >
            <Search />
            <span>Search</span>
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
        ref={input}
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
  const { data: status } = useStatus()
  const progress = status?.syncProgress
  if (!status?.authenticated || !progress?.active) return null

  const catalog = progress.catalog
  const error = progress.error
  const retryAt = progress.retryAt
  const isRetrying = progress.phase === "retrying"
  const phase = isRetrying
    ? "Synchronization paused"
    : progress.phase === "catalog"
      ? "Loading library"
      : "Refreshing library"
  const current = catalog.processed
  const total = catalog.total
  const determinate = progress.phase === "catalog"
  const retryLabel = retryAt
    ? `Retry scheduled for ${new Date(retryAt * 1_000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
    : "Retry scheduled automatically"
  const detail = isRetrying
    ? retryLabel
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

function UserMenu() {
  const { data: status } = useStatus()
  const logout = useLogout()
  const name = status?.userName ?? "Signed in"

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
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
                  .catch((error: Error) => toast.error(error.message))
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

type AppSidebarProps = Pick<ComponentProps<typeof Sidebar>, "onPointerEnter" | "onPointerLeave">

export function AppSidebar(props: AppSidebarProps = {}) {
  const location = useLocation()
  const cache = useQueryClient()
  const { data: status } = useStatus()
  const account = collectionAccountKey(status)
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
  const detailOrigin = readDetailNavigationState(location.state)
  const originUrl = detailOrigin?.from.startsWith("/library")
    ? new URL(detailOrigin.from, "https://mediaflick.invalid")
    : null
  const activeLibraryNav = (title: string) => {
    if (location.pathname === "/library") {
      if (title === "Movies") return libraryKind(libraryParams) === "Movie"
      if (title === "Series") return libraryKind(libraryParams) === "Series"
      return (
        title === "Favorites"
        && libraryParams.get("favorite") === "true"
        && !libraryParams.has("kind")
      )
    }
    if (!detailId) return false
    if (
      title === "Favorites"
      && originUrl?.searchParams.get("favorite") === "true"
      && !originUrl.searchParams.has("kind")
    ) return true
    if (title === "Movies") return detail.data?.kind === "Movie"
    if (title === "Series") {
      return detail.data != null && detail.data.kind !== "Movie"
    }
    return false
  }
  const { data: companion } = useCompanion()
  const { data: collectionSettings } = useCollectionSettings()
  const companionSeerr =
    companion?.compatible && companion.info?.capabilities.includes("seerr")
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
    <Sidebar collapsible="icon" className="app-sidebar-container" {...props}>
      <SidebarHeader>
        <div className="flex items-center gap-2">
          <Link
            to="/"
            className="flex min-w-0 items-center gap-2 font-medium"
          >
            <div className="flex aspect-square size-8 shrink-0 items-center justify-center rounded-media bg-primary text-primary-foreground shadow-lg shadow-primary/25">
              <Film className="size-4" />
            </div>
            {/* "Flick" carries the accent, so the wordmark states the palette
                the rest of the shell is built from. */}
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
                    isActive={
                      item.title === "Home"
                        ? location.pathname === "/"
                        : activeLibraryNav(item.title)
                    }
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
        <UserMenu />
      </SidebarFooter>
    </Sidebar>
  )
}
