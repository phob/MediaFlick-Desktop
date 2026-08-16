/** Routes whose detail pages remain part of the same top-level destination. */
const NESTED_SIDEBAR_ROUTES = new Set(["/discover", "/settings"])

export function isSidebarRouteActive(target: string, pathname: string) {
  return pathname === target
    || (NESTED_SIDEBAR_ROUTES.has(target) && pathname.startsWith(`${target}/`))
}

/** The sidebar search mirrors the URL only while the library owns the view. */
export function librarySearchFromLocation(pathname: string, search: string) {
  return pathname === "/library"
    ? new URLSearchParams(search).get("search") ?? ""
    : ""
}

export interface DetailNavigationState {
  from: string
  label: string
}

/** A directly opened library detail still has a useful, visible way out. */
export function defaultDetailNavigationState(kind: string): DetailNavigationState {
  return {
    from: `/library?kind=${kind === "Movie" ? "Movie" : "Series"}`,
    label: "Back to library",
  }
}

type LocationLike = {
  pathname: string
  search?: string
  state?: unknown
}

/** A detail page remembers the browsing surface that opened it. */
export function detailNavigationState(location: LocationLike): DetailNavigationState {
  const inherited = readDetailNavigationState(location.state)
  if (location.pathname.startsWith("/item/") && inherited) return inherited

  const from = `${location.pathname}${location.search ?? ""}`
  const label = location.pathname === "/"
    ? "Back to home"
    : location.pathname === "/library"
      ? "Back to library"
      : location.pathname.startsWith("/discover")
        ? "Back to discovery"
        : location.pathname === "/calendar"
          ? "Back to releases"
          : location.pathname === "/requests"
            ? "Back to requests"
            : "Back"
  return { from, label }
}

/** Accept navigation state only when it is a safe app-internal target. */
export function readDetailNavigationState(value: unknown): DetailNavigationState | null {
  if (!value || typeof value !== "object") return null
  const state = value as Partial<DetailNavigationState>
  if (
    typeof state.from !== "string"
    || !state.from.startsWith("/")
    || state.from.startsWith("//")
    || typeof state.label !== "string"
    || !state.label.trim()
  ) return null
  return { from: state.from, label: state.label }
}
