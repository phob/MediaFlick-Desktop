const APP_NAME = "MediaFlick"

export interface WindowTitleContext {
  authenticated: boolean
  itemTitle?: string | null
}

export function windowTitle(pathname: string, context: WindowTitleContext): string {
  if (pathname === "/") {
    return context.authenticated ? `Home — ${APP_NAME}` : `Sign in — ${APP_NAME}`
  }
  const section =
    pathname.startsWith("/settings") ? "Settings"
    : pathname.startsWith("/item/") ? null
    : pathname.startsWith("/library") ? "Library"
    : pathname.startsWith("/calendar") ? "Releases"
    : pathname.startsWith("/discover") ? "Discover"
    : pathname.startsWith("/requests") ? "Requests"
    : null
  if (section) return `${section} — ${APP_NAME}`
  return context.itemTitle ? `${context.itemTitle} — ${APP_NAME}` : APP_NAME
}
