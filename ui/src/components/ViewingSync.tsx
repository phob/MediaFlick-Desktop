import { useQuery } from "@tanstack/react-query"
import { useEffect, useRef } from "react"
import { useLocation, useNavigate } from "react-router-dom"
import { api } from "@/lib/api"
import { collectionAccountKey, useStatus } from "@/lib/queries"
import { DEFAULT_VIEWING, useViewing } from "@/lib/viewing"

export function ViewingSync() {
  const viewing = useViewing()
  const { data: status } = useStatus()
  const account = collectionAccountKey(status)
  const history = useQuery({ queryKey: ["browsing", account], queryFn: api.browsing, enabled: Boolean(status?.authenticated) })
  const location = useLocation()
  const navigate = useNavigate()
  const initialized = useRef<string | null>(null)
  useEffect(() => {
    const value = viewing.data ?? DEFAULT_VIEWING
    const root = document.documentElement
    root.style.fontSize = `${16 * value.textScale / 100}px`
    root.style.setProperty("--poster-width", `${value.posterSize}px`)
    root.style.setProperty("--poster-height", `${value.posterSize * 1.5}px`)
    root.style.setProperty("--card-height", `${value.posterSize * 1.5 + 54 * value.textScale / 100}px`)
    window.dispatchEvent(new Event("resize"))
  }, [viewing.data])
  useEffect(() => {
    if (!status?.authenticated) { initialized.current = null; return }
    if (!viewing.data || !history.data) return
    if (initialized.current !== account) {
      initialized.current = account
      if (location.pathname === "/" && !location.search) {
        const destinations = {home:"/", movies:"/library?kind=Movie", series:"/library?kind=Series", calendar:"/calendar", last:history.data.last ?? "/"}
        const destination = destinations[viewing.data.startupDestination]
        if (destination !== "/") { void navigate(destination, {replace:true}); return }
      }
    }
    const route = location.pathname + location.search
    if (!["/", "/library", "/calendar", "/discover", "/requests", "/collections"].includes(location.pathname) && !location.pathname.startsWith("/collections/")) return
    const timer = window.setTimeout(() => { void api.saveBrowsing("last", route).catch(() => {}) }, 500)
    return () => window.clearTimeout(timer)
  }, [account, history.data, location.pathname, location.search, navigate, status?.authenticated, viewing.data])
  return null
}
