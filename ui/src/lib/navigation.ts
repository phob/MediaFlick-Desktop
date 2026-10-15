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

/** Without navigation state, a title goes back to its library and anything else to Home. */
export function defaultDetailNavigationState(
  item: AnyItemDetail,
  libraries: readonly Library[],
): DetailNavigationState {
  if (item.layout === "generic") return HOME_STATE
  const library = libraries.find((candidate) => candidate.id === item.libraryId)
  return library
    ? { from: libraryPath(library.id), label: `Back to ${library.name}` }
    : HOME_STATE
}

type LocationLike = {
  pathname: string
  search?: string
  state?: JsonValue
}

/** `libraryName` names the library page the location shows, when it shows one. */
export function detailNavigationState(
  location: LocationLike,
  libraryName: string | null = null,
): DetailNavigationState {
  const inherited = readDetailNavigationState(location.state)
  if (location.pathname.startsWith("/item/") && inherited) return inherited

  const from = `${location.pathname}${location.search ?? ""}`
  if (libraryIdFromPath(location.pathname) !== null) {
    return { from, label: libraryName ? `Back to ${libraryName}` : "Back to library" }
  }
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
