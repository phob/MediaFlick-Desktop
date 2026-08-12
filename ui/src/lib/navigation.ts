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
