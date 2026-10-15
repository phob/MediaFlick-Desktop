const NESTED_SIDEBAR_ROUTES = new Set(["/discover", "/settings"])

export function isSidebarRouteActive(target: string, pathname: string) {
  return pathname === target
    || (NESTED_SIDEBAR_ROUTES.has(target) && pathname.startsWith(`${target}/`))
}

export function librarySearchFromLocation(pathname: string, search: string) {
  return pathname === "/library"
    ? new URLSearchParams(search).get("search") ?? ""
    : ""
}

export interface DetailNavigationState {
  from: string
  label: string
}

export function defaultDetailNavigationState(item: AnyItemDetail): DetailNavigationState {
  if (item.layout === "generic") return { from: "/", label: "Back to home" }
  return {
    from: `/library?kind=${titleBrowseKind(item.kind)}`,
    label: "Back to library",
  }
}

type LocationLike = {
  pathname: string
  search?: string
  state?: JsonValue
}

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

export function readDetailNavigationState(value: JsonValue | undefined): DetailNavigationState | null {
  if (!isJsonObject(value)) return null
  const from = jsonString(value.from)
  const label = jsonString(value.label)
  if (
    !from
    || !from.startsWith("/")
    || from.startsWith("//")
    || !label?.trim()
  ) return null
  return { from, label }
}
import { isJsonObject, jsonString, type JsonValue } from "./json.ts"
import { titleBrowseKind } from "./libraries.ts"
import type { AnyItemDetail } from "./api/types.ts"
