import { useState } from "react"
import { SeerrStatusBadge } from "@/components/seerr/SeerrStatusBadge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Label } from "@/components/ui/label"
import { Skeleton } from "@/components/ui/skeleton"
import type { SeerrCapability, SeerrResult, SeerrSeason } from "@/lib/api"
import { useSeerrMedia, useSeerrRequest, useSeerrStatus } from "@/lib/queries"

/** A season already here, on its way, or blocked is not one to ask for again. */
function isRequestable(season: SeerrSeason) {
  return season.status === "unknown"
}

function seasonLabel(season: SeerrSeason) {
  const name = season.name?.trim()
  const label =
    name && name !== `Season ${season.seasonNumber}` ? name : `Season ${season.seasonNumber}`
  return season.episodeCount
    ? `${label} · ${season.episodeCount} ${season.episodeCount === 1 ? "episode" : "episodes"}`
    : label
}

/**
 * Season picking, only where it applies: a movie has none, and an instance with
 * `partialRequestsEnabled` off takes the whole show or nothing.
 */
function SeasonPicker({
  seasons,
  selected,
  onToggle,
}: {
  seasons: SeerrSeason[]
  selected: Set<number>
  onToggle: (seasonNumber: number) => void
}) {
  return (
    <div className="flex max-h-56 flex-col gap-1 overflow-y-auto rounded-md border p-2">
      {seasons.map((season) => (
        <Label
          key={season.seasonNumber}
          className="flex items-center justify-between gap-3 rounded-sm px-2 py-1.5 font-normal hover:bg-accent has-[input:disabled]:opacity-60"
        >
          <span className="flex items-center gap-2">
            <input
              type="checkbox"
              className="size-4 accent-primary"
              checked={selected.has(season.seasonNumber)}
              disabled={!isRequestable(season)}
              onChange={() => onToggle(season.seasonNumber)}
            />
            <span className="text-sm">{seasonLabel(season)}</span>
          </span>
          <SeerrStatusBadge status={season.status} />
        </Label>
      ))}
    </div>
  )
}

/**
 * The request confirmation. Mounted only while open, so closing it discards the
 * season selection rather than leaving it to be reset.
 *
 * The dialog is shown even to a user whose requests are approved automatically
 * (chunk 11's resolved default) — what changes is the outcome the toast
 * reports, which comes from the status Seerr answers with.
 */
export function RequestDialog({
  result,
  onClose,
}: {
  result: SeerrResult
  onClose: () => void
}) {
  const status = useSeerrStatus()
  const isSeries = result.mediaType === "tv"
  // Only a series needs the detail call: it is where the seasons come from.
  const detail = useSeerrMedia(result.mediaType, result.tmdbId, isSeries)
  const request = useSeerrRequest()
  const [is4k, setIs4k] = useState(false)
  // `null` is "untouched", which is what lets the default — everything Seerr
  // does not already have — follow the seasons as they load.
  const [chosen, setChosen] = useState<Set<number> | null>(null)

  const capability: SeerrCapability | undefined = status.data?.capabilities?.[result.mediaType]
  const capability4k: SeerrCapability | undefined =
    status.data?.capabilities?.[isSeries ? "tv4k" : "movie4k"]
  const partialRequests = status.data?.instance.partialRequestsEnabled ?? false

  const seasons = detail.data?.seasons ?? []
  const requestable = seasons.filter(isRequestable)
  const selected = chosen ?? new Set(requestable.map((season) => season.seasonNumber))

  const quota = is4k ? null : status.data?.quota?.[result.mediaType]
  const overQuota = Boolean(quota?.restricted)
  const nothingLeft = isSeries && detail.isSuccess && requestable.length === 0
  const permitted = Boolean(is4k ? capability4k?.request : capability?.request)
  const canSubmit =
    permitted && !overQuota && !nothingLeft && (!isSeries || !partialRequests || selected.size > 0)

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            Request {result.title}
            {result.year ? ` (${result.year})` : ""}
          </DialogTitle>
          <DialogDescription>
            {capability?.autoApprove && !is4k
              ? "Your requests are approved automatically."
              : "Your Seerr administrator approves requests before they are downloaded."}
          </DialogDescription>
        </DialogHeader>

        {isSeries && detail.isPending && <Skeleton className="h-24" />}
        {isSeries && detail.error && (
          <p className="text-sm text-destructive">{detail.error.message}</p>
        )}
        {isSeries && detail.isSuccess && partialRequests && requestable.length > 0 && (
          <SeasonPicker
            seasons={seasons}
            selected={selected}
            onToggle={(seasonNumber) =>
              setChosen(() => {
                const next = new Set(selected)
                if (!next.delete(seasonNumber)) next.add(seasonNumber)
                return next
              })
            }
          />
        )}
        {nothingLeft && (
          <p className="text-sm text-muted-foreground">
            Seerr already has every season of this show.
          </p>
        )}

        {/* Only offered where the instance has 4K on *and* the user's own
            permission mask carries the 4K bit for this media type. */}
        {capability4k?.request && (
          <Label className="font-normal">
            <input
              type="checkbox"
              className="size-4 accent-primary"
              checked={is4k}
              onChange={(event) => setIs4k(event.target.checked)}
            />
            Request in 4K
          </Label>
        )}

        {quota?.limit != null && (
          <p className="text-xs text-muted-foreground">
            {quota.remaining ?? 0} of {quota.limit} requests left
            {quota.days ? ` in the last ${quota.days} days` : ""}.
          </p>
        )}
        {overQuota && (
          <p className="text-sm text-destructive">You have used your request quota for now.</p>
        )}
        {!permitted && (
          <p className="text-sm text-destructive">
            Your Seerr account is not allowed to request this.
          </p>
        )}

        <DialogFooter>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={!canSubmit || request.isPending}
            onClick={() =>
              request.mutate(
                {
                  mediaType: result.mediaType,
                  tmdbId: result.tmdbId,
                  // Omitted for a movie, and for an instance that does not do
                  // partial requests: Seerr then takes the whole show.
                  seasons:
                    isSeries && partialRequests ? [...selected].sort((a, b) => a - b) : undefined,
                  is4k,
                },
                { onSuccess: onClose },
              )
            }
          >
            {request.isPending ? "Requesting…" : "Request"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
