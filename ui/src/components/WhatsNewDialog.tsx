import { useState, type ReactNode } from "react"
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
import type { CompanionStatus } from "@/lib/api"
import { companionMissing, useCompanionCapabilities } from "@/lib/companion"
import { useCompanion, useStatus } from "@/lib/queries"
import { useCloseWhatsNew, useWhatsNew } from "@/lib/whats-new"

type Entry = {
  title: string
  text: string
  /** Where the user finds it: the feature's own page or its settings. */
  link?: { to: string; label: string }
}

/** A Companion feature, linked only once the server's Companion offers it. */
type CompanionEntry = Entry & { capability: string }

const COMPANION_FEATURES: readonly CompanionEntry[] = [
  {
    title: "Discover and request",
    text: "Browse trending, popular and upcoming films and series, filter by genre, decade, studio or network, and request what you don't have through Seerr. Requests follows each one until it reaches your library.",
    link: { to: "/discover", label: "Discover" },
    capability: "seerr",
  },
  {
    title: "Upcoming releases",
    text: "See what Sonarr and Radarr have coming, by day or by month, and what is already downloaded.",
    link: { to: "/calendar", label: "Releases" },
    capability: "calendar",
  },
  {
    title: "Ratings on every card",
    text: "Pick the scores your cards show from MDBList, such as IMDb, Rotten Tomatoes, Metacritic or Letterboxd, in Discover too.",
    link: { to: "/settings/appearance", label: "Rating sources" },
    capability: "ratings-v1",
  },
  {
    title: "Movie franchises",
    text: "Every TMDB movie franchise you own a part of, in release order, with the films you're missing.",
    link: { to: "/collections", label: "Collections" },
    capability: "franchise-memberships-v1",
  },
]

const MEDIAFLICK_FEATURES: readonly Entry[] = [
  {
    title: "Instant search",
    text: "Your library is kept on this device, so search answers as you type, across titles, cast and genres.",
  },
  {
    title: "Spoiler protection",
    text: "Hide the images, names and summaries of episodes you haven't watched.",
    link: { to: "/settings/viewing", label: "Viewing settings" },
  },
  {
    title: "Letterboxd",
    text: "Connect Letterboxd profiles to see their ratings and reviews on movie pages.",
    link: { to: "/settings/integrations/letterboxd", label: "Letterboxd settings" },
  },
  {
    title: "Your Home",
    text: "Add genre shelves and a Because you watched shelf, and arrange Home the way you like.",
    link: { to: "/settings/home", label: "Home settings" },
  },
  {
    title: "Trailers and previews",
    text: "Home opens on a billboard that plays trailers, and posters show a preview when you hover them.",
    link: { to: "/settings/appearance", label: "Appearance settings" },
  },
]

const SINCE_0_1_6: readonly Entry[] = [
  {
    title: "Signed in again",
    text: "MediaFlick now signs in to Jellyfin itself, so this device asked you to sign in once more.",
  },
  {
    title: "MPC-HC is gone",
    text: "If you played with MPC-HC, MediaFlick now uses its built-in player. You can choose External mpv instead.",
    link: { to: "/settings/client/player", label: "Player settings" },
  },
  {
    title: "Jellyfin versions",
    text: "MediaFlick needs Jellyfin 10.11.0 or newer. MediaFlick Companion needs Jellyfin 12.0.0 or newer.",
    link: { to: "/settings/integrations/companion", label: "Companion settings" },
  },
  {
    title: "Music, audiobooks and books",
    text: "Albums and audiobooks play in the app, books open in its reader, and every Jellyfin library has its own page in the sidebar.",
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
  const capabilities = useCompanionCapabilities()
  const close = useCloseWhatsNew()
  const [closed, setClosed] = useState(false)
  if (!signedIn || !whatsNew.data?.pending) return null

  const dismiss = () => {
    setClosed(true)
    close.mutate()
  }
  const companionFeatures = COMPANION_FEATURES.map(({ capability, link, ...entry }) => (
    capabilities.has(capability) ? { ...entry, link } : entry
  ))

  return (
    <Dialog open={!closed} onOpenChange={(open) => { if (!open) dismiss() }}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>What's new since MediaFlick 0.1.6</DialogTitle>
          <DialogDescription>
            MediaFlick now has its own interface instead of Jellyfin Web, with things Jellyfin Web doesn't offer.
          </DialogDescription>
        </DialogHeader>
        <Section id="whats-new-with-companion" title="With MediaFlick Companion">
          <Entries entries={companionFeatures} onFollow={dismiss} />
          <CompanionNote companion={companion} administrator={Boolean(status?.administrator)} />
        </Section>
        <Section id="whats-new-mediaflick" title="Built into MediaFlick">
          <Entries entries={MEDIAFLICK_FEATURES} onFollow={dismiss} />
        </Section>
        <Section id="whats-new-since" title="Changed since 0.1.6">
          <Entries entries={SINCE_0_1_6} onFollow={dismiss} />
        </Section>
        <DialogFooter>
          <Button onClick={dismiss}>Got it</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section aria-labelledby={id} className="grid gap-3">
      <h3 id={id} className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{title}</h3>
      {children}
    </section>
  )
}

function Entries({ entries, onFollow }: { entries: readonly Entry[]; onFollow: () => void }) {
  return (
    <ul className="grid gap-4">
      {entries.map((entry) => (
        <li key={entry.title}>
          <h4 className="font-medium">{entry.title}</h4>
          <p className="mt-1 text-sm text-muted-foreground">{entry.text}</p>
          {entry.link && (
            <Button asChild variant="link" className="h-auto p-0">
              <Link to={entry.link.to} onClick={onFollow}>{entry.link.label}</Link>
            </Button>
          )}
        </li>
      ))}
    </ul>
  )
}

/**
 * Says whether this server has the Companion the features above come with,
 * and who can add it where it could run.
 */
function CompanionNote({ companion, administrator }: { companion: CompanionStatus | undefined; administrator: boolean }) {
  if (companion?.compatible) {
    return <p className="text-sm text-muted-foreground">Your Jellyfin server has MediaFlick Companion.</p>
  }
  if (companion?.serverTooOld) {
    return (
      <p className="text-sm text-muted-foreground">
        These come with MediaFlick Companion, which needs Jellyfin 12.0.0 or newer.
      </p>
    )
  }
  if (!companionMissing(companion)) {
    return <p className="text-sm text-muted-foreground">These come with MediaFlick Companion on your Jellyfin server.</p>
  }
  if (!administrator) {
    return (
      <p className="text-sm text-muted-foreground">
        Your server administrator can add MediaFlick Companion to turn these on.
      </p>
    )
  }
  return (
    <section aria-labelledby="whats-new-companion" className="grid gap-2 rounded-md border p-4">
      <h4 id="whats-new-companion" className="font-medium">Add MediaFlick Companion</h4>
      <p className="text-sm text-muted-foreground">
        You administer this Jellyfin server. Install the MediaFlick Companion plugin to turn these on.
      </p>
      <div>
        <CompanionGuideLink />
      </div>
    </section>
  )
}
