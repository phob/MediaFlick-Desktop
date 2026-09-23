import {
  House,
  Layers,
  Link,
  Monitor,
  Palette,
  Play,
  Plug,
  SlidersHorizontal,
  type LucideIcon,
} from "lucide-react"
import { Fragment } from "react"
import { Link as RouterLink, Navigate, Route, Routes, useLocation } from "react-router-dom"
import SettingsDraftGuard from "@/components/SettingsDraftGuard"
import { accountKey, useStatus } from "@/lib/queries"
import CollectionSettingsPage from "@/routes/CollectionSettings"
import ApplicationSettings from "./settings/ApplicationSettings"
import { Appearance } from "./settings/AppearanceSettings"
import { CompanionIntegration } from "./settings/CompanionSettings"
import HomeSettings from "./settings/HomeSettings"
import LetterboxdSettings from "./settings/LetterboxdSettings"
import PlaybackSettings from "./settings/PlaybackSettings"
import PlayerSettings from "./settings/PlayerSettings"
import ViewingSettings from "./settings/ViewingSettings"

type SettingsPage = {
  to: string
  title: string
  icon: LucideIcon
  group?: string
  signedIn?: boolean
}

const NAVIGATION: SettingsPage[] = [
  { to: "/settings/client/player", title: "Player", icon: Play, group: "Client" },
  { to: "/settings/client/playback", title: "Playback", icon: SlidersHorizontal, group: "Client" },
  { to: "/settings/client/application", title: "Application", icon: Monitor, group: "Client" },
  { to: "/settings/viewing", title: "Viewing", icon: SlidersHorizontal, signedIn: true, group: "Account" },
  { to: "/settings/home", title: "Home", icon: House, signedIn: true, group: "Account" },
  { to: "/settings/appearance", title: "Appearance", icon: Palette, signedIn: true, group: "Account" },
  { to: "/settings/collections", title: "Collections", icon: Layers, signedIn: true, group: "Account" },
  {
    to: "/settings/integrations/companion",
    title: "MediaFlick Companion",
    icon: Plug,
    signedIn: true,
    group: "Integrations",
  },
  { to: "/settings/integrations/letterboxd", title: "Letterboxd", icon: Link, signedIn: true, group: "Integrations" },
]

function SettingsNavigation() {
  const location = useLocation()
  const { data: status } = useStatus()
  return (
    <nav className="settings-navigation" aria-label="Settings navigation">
      <span className="settings-nav-label">Settings</span>
      {NAVIGATION.map((item, index) => {
        const active = location.pathname.startsWith(item.to)
        const previousGroup = NAVIGATION[index - 1]?.group
        return (
          <Fragment key={item.to}>
            {item.group && item.group !== previousGroup && <span className="settings-nav-group">{item.group}</span>}
            <RouterLink to={item.to} data-active={active} aria-disabled={item.signedIn && !status?.authenticated}>
              <item.icon />
              <span>{item.title}</span>
            </RouterLink>
          </Fragment>
        )
      })}
    </nav>
  )
}

export default function Settings() {
  const { data: status } = useStatus()
  return (
    <SettingsDraftGuard key={accountKey(status)}>
      <div className="settings-layout">
        <SettingsNavigation />
        <main className="settings-main">
          <Routes>
            <Route index element={<Navigate to="/settings/client/player" replace />} />
            <Route path="client/player" element={<PlayerSettings />} />
            <Route path="client/playback" element={<PlaybackSettings />} />
            <Route path="client/application" element={<ApplicationSettings />} />
            <Route path="viewing" element={<ViewingSettings />} />
            <Route path="home" element={<HomeSettings />} />
            <Route path="appearance" element={<Appearance />} />
            <Route path="collections" element={<CollectionSettingsPage />} />
            <Route path="integrations/companion" element={<CompanionIntegration />} />
            <Route path="integrations/letterboxd" element={<LetterboxdSettings />} />
            <Route path="*" element={<Navigate to="/settings" replace />} />
          </Routes>
        </main>
      </div>
    </SettingsDraftGuard>
  )
}
