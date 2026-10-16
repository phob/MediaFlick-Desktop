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

const HOME_STATE: DetailNavigationState = { from: "/", label: "Back to home" }

function backToLibrary(id: string, libraries: readonly Library[]) {
  const name = libraries.find((library) => library.id === id)?.name
  return name ? `Back to ${name}` : "Back to library"
}

export function defaultDetailNavigationState(
  item: AnyItemDetail,
  libraries: readonly Library[],
): DetailNavigationState {
  if (item.layout === "generic" || !item.libraryId) return HOME_STATE
  return { from: libraryPath(item.libraryId), label: backToLibrary(item.libraryId, libraries) }
}

type LocationLike = {
  pathname: string
  search?: string
  state?: JsonValue
}

export function detailNavigationState(
  location: LocationLike,
  libraries: readonly Library[] = [],
): DetailNavigationState {
  const inherited = readDetailNavigationState(location.state)
  if (location.pathname.startsWith("/item/") && inherited) return inherited

  const from = `${location.pathname}${location.search ?? ""}`
  const libraryId = libraryIdFromPath(location.pathname)
  if (libraryId !== null) return { from, label: backToLibrary(libraryId, libraries) }
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
import { libraryIdFromPath, libraryPath } from "./libraries.ts"
import type { AnyItemDetail, Library } from "./api/types.ts"
