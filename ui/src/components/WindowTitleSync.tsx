import { useSpoilerProtection } from "@/lib/viewing"
import { useEffect } from "react"
import { matchPath, useLocation } from "react-router-dom"
import { useItem, useStatus } from "@/lib/queries"
import { windowTitle } from "@/lib/window-title"

export function WindowTitleSync() {
  const location = useLocation()
  const { data: status } = useStatus()
  // Shares the item-detail cache entry, so an open detail page adds no request.
  const itemMatch = matchPath("/item/:id", location.pathname)
  const { data: item } = useItem(itemMatch?.params.id)

  const hidden = useSpoilerProtection(item)
  useEffect(() => {
    document.title = windowTitle(location.pathname, {
      authenticated: Boolean(status?.authenticated),
      itemTitle: hidden ? "Unwatched episode" : item?.name,
    })
  }, [location.pathname, status?.authenticated, item?.name, hidden])

  return null
}
