import { useEffect } from "react"
import { useSettings } from "@/lib/queries"

/** Applies the saved appearance to the document root, where the theme tokens read it. */
export function AppearanceSync() {
  const { data: settings } = useSettings()
  useEffect(() => {
    const appearance = settings?.appearance
    if (!appearance) return
    const root = document.documentElement
    root.dataset.accent = appearance.accent
    root.dataset.density = appearance.density
    root.dataset.reducedMotion = String(appearance.reducedMotion)
    root.dataset.cardPreviews = String(appearance.cardPreviews)
    root.dataset.mediaInfo = String(appearance.showMediaInfo)
    root.style.setProperty("--artwork-intensity", String(appearance.artworkIntensity / 100))
    root.style.setProperty("--backdrop-intensity", String(appearance.backdropIntensity / 100))
  }, [settings?.appearance])
  return null
}
