import { useState } from "react"
import { Link } from "react-router-dom"
import { CompanionGuideLink } from "@/components/CompanionGuideLink"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { companionMissing } from "@/lib/companion"
import { useCompanion, useStatus } from "@/lib/queries"
import { useCloseWhatsNew, useWhatsNew } from "@/lib/whats-new"

type Change = {
  title: string
  text: string
  settings?: { to: string; label: string }
}

const CHANGES: readonly Change[] = [
  {
    title: "Its own interface",
    text: "MediaFlick now has its own interface instead of Jellyfin Web.",
    settings: { to: "/settings/appearance", label: "Appearance settings" },
  },
  {
    title: "Signed in again",
    text: "MediaFlick now signs in to Jellyfin itself, so this device asked you to sign in once more.",
  },
  {
    title: "Server dashboard in your browser",
    text: "Right-click anywhere and choose Open Jellyfin dashboard to manage your server in your browser.",
  },
  {
    title: "MPC-HC is gone",
    text: "If you played with MPC-HC, MediaFlick now uses its built-in player. You can choose External mpv instead.",
    settings: { to: "/settings/client/player", label: "Player settings" },
  },
  {
    title: "Jellyfin versions",
    text: "MediaFlick needs Jellyfin 10.11.0 or newer. MediaFlick Companion needs Jellyfin 12.0.0 or newer.",
    settings: { to: "/settings/integrations/companion", label: "Companion settings" },
  },
  {
    title: "Music, audiobooks and books",
    text: "Albums and audiobooks play in the app, books open in its reader, and every Jellyfin library has its own page in the sidebar.",
    settings: { to: "/settings/home", label: "Home settings" },
  },
]

/**
 * The one-time What's new for a device updated from 0.1.6. It holds no
 * settings and closes like any dialog; closing it, or following one of its
 * links, records it as seen.
 */
export function WhatsNewDialog() {
  const { data: status } = useStatus()
  const signedIn = Boolean(status?.authenticated)
  const whatsNew = useWhatsNew(signedIn)
  const { data: companion } = useCompanion()
  const close = useCloseWhatsNew()
  const [closed, setClosed] = useState(false)
  if (!signedIn || !whatsNew.data?.pending) return null

  const dismiss = () => {
    setClosed(true)
    close.mutate()
  }
  const missing = companionMissing(companion)

  return (
    <Dialog open={!closed} onOpenChange={(open) => { if (!open) dismiss() }}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>What's new since MediaFlick 0.1.6</DialogTitle>
          <DialogDescription>MediaFlick changed a lot. Here is what is different.</DialogDescription>
        </DialogHeader>
        <ul className="grid gap-4">
          {CHANGES.map((change) => (
            <li key={change.title}>
              <h3 className="font-medium">{change.title}</h3>
              <p className="mt-1 text-sm text-muted-foreground">{change.text}</p>
              {change.settings && (
                <Button asChild variant="link" className="h-auto p-0">
                  <Link to={change.settings.to} onClick={dismiss}>{change.settings.label}</Link>
                </Button>
              )}
            </li>
          ))}
        </ul>
        {missing && status?.administrator && (
          <section aria-labelledby="whats-new-companion" className="grid gap-2 rounded-md border p-4">
            <h3 id="whats-new-companion" className="font-medium">Add MediaFlick Companion</h3>
            <p className="text-sm text-muted-foreground">
              You administer this Jellyfin server. The MediaFlick Companion plugin adds Discover and requests through
              Seerr, upcoming releases from Sonarr and Radarr, MDBList ratings and TMDB movie franchises.
            </p>
            <div>
              <CompanionGuideLink />
            </div>
          </section>
        )}
        {missing && !status?.administrator && (
          <p className="text-sm text-muted-foreground">
            Your server administrator can add MediaFlick Companion for Discover, requests and ratings.
          </p>
        )}
        <DialogFooter>
          <Button onClick={dismiss}>Got it</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
