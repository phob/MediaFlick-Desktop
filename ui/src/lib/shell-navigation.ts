import { useEffect, useRef } from "react"
import { useLocation, useNavigate } from "react-router-dom"
import { readShellEvent } from "./shell-events"

export const SETTINGS_PATH = "/settings/client/player"

let openSettingsBeforeBridge = false

function noteOpenSettings(event: Event) {
  if (readShellEvent(event)?.type === "open-settings") openSettingsBeforeBridge = true
}

export function listenForOpenSettingsBeforeBridge() {
  window.addEventListener("mediaflick-desktop-shell", noteOpenSettings)
  return () => {
    window.removeEventListener("mediaflick-desktop-shell", noteOpenSettings)
    openSettingsBeforeBridge = false
  }
}

export function useShellNavigation() {
  const navigate = useNavigate()
  const location = useLocation()
  const pathnameRef = useRef(location.pathname)

  useEffect(() => {
    pathnameRef.current = location.pathname
  }, [location.pathname])

  useEffect(() => {
    const openSettings = () => {
      if (pathnameRef.current !== SETTINGS_PATH) void navigate(SETTINGS_PATH)
    }
    const receive = (event: Event) => {
      if (readShellEvent(event)?.type === "open-settings") openSettings()
    }
    window.addEventListener("mediaflick-desktop-shell", receive)
    window.removeEventListener("mediaflick-desktop-shell", noteOpenSettings)
    if (openSettingsBeforeBridge) {
      openSettingsBeforeBridge = false
      openSettings()
    }
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [navigate])
}
