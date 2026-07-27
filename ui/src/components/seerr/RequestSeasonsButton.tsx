import { Plus } from "lucide-react"
import { useState } from "react"
import { RequestDialog } from "@/components/seerr/RequestDialog"
import { Button } from "@/components/ui/button"
import type { ItemDetail } from "@/lib/api"
import { useSeerrMedia, useSeerrStatus } from "@/lib/queries"

/**
 * "Request season" on a series the library only partly has.
 *
 * The series is matched to Seerr by its own TMDB id — the same (kind, TMDB id)
 * join the search results use, from the other direction — so a show with no
 * TMDB id, or one Seerr already has in full, offers nothing at all rather than
 * a button that would open an empty dialog.
 */
export function RequestSeasonsButton({ item }: { item: ItemDetail }) {
  const status = useSeerrStatus()
  const [requesting, setRequesting] = useState(false)
  const tmdbId = Number(item.providerIds?.tmdb ?? "")
  const enabled =
    item.kind === "Series" && Boolean(status.data?.linked) && Number.isFinite(tmdbId) && tmdbId > 0
  const media = useSeerrMedia("tv", tmdbId, enabled)

  const missing = media.data?.seasons.filter((season) => season.status === "unknown") ?? []
  if (!enabled || !media.data || !missing.length) return null
  if (!status.data?.capabilities?.tv.request && !status.data?.capabilities?.tv4k.request) {
    return null
  }

  return (
    <>
      <Button variant="secondary" size="lg" onClick={() => setRequesting(true)}>
        <Plus className="size-4" />
        {missing.length === 1 ? `Request season ${missing[0].seasonNumber}` : "Request seasons"}
      </Button>
      {requesting && (
        <RequestDialog result={media.data} onClose={() => setRequesting(false)} />
      )}
    </>
  )
}
