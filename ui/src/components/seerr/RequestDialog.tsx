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
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import type { SeerrCapability, SeerrResult, SeerrSeason } from "@/lib/api"
import {
  useSeerrMedia,
  useSeerrRequest,
  useSeerrRequestOptions,
  useSeerrStatus,
} from "@/lib/queries"

/** A season already here, on its way, or blocked is not one to ask for again. */
function isRequestable(season: SeerrSeason, is4k: boolean) {
  return season[is4k ? "status4k" : "status"] === "unknown"
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
  is4k,
  onToggle,
}: {
  seasons: SeerrSeason[]
  selected: Set<number>
  is4k: boolean
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
              disabled={!isRequestable(season, is4k)}
              onChange={() => onToggle(season.seasonNumber)}
            />
            <span className="text-sm">{seasonLabel(season)}</span>
          </span>
          <SeerrStatusBadge status={season[is4k ? "status4k" : "status"]} />
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
  const [qualityChoice, setQualityChoice] = useState<string | null>(null)

  const capability: SeerrCapability | undefined = status.data?.capabilities?.[result.mediaType]
  const capability4k: SeerrCapability | undefined =
    status.data?.capabilities?.[isSeries ? "tv4k" : "movie4k"]
  const selectedCapability = is4k ? capability4k : capability
  const permitted = Boolean(selectedCapability?.request)
  const advancedRequest = Boolean(status.data?.capabilities?.advancedRequest)
  const requestOptions = useSeerrRequestOptions(
    result.mediaType,
    is4k,
    advancedRequest && permitted,
  )
  const partialRequests = status.data?.instance.partialRequestsEnabled ?? false

  const seasons = detail.data?.seasons ?? []
  const requestable = seasons.filter((season) => isRequestable(season, is4k))
  const selected = chosen ?? new Set(requestable.map((season) => season.seasonNumber))

  const quota = is4k ? null : status.data?.quota?.[result.mediaType]
  const overQuota = Boolean(quota?.restricted)
  const mediaStatus = is4k ? result.status4k : result.status
  const nothingLeft = isSeries
    ? detail.isSuccess && requestable.length === 0
    : mediaStatus !== "unknown"
  const qualityLoading = advancedRequest && permitted && requestOptions.isPending
  const canSubmit =
    permitted &&
    !overQuota &&
    !nothingLeft &&
    !qualityLoading &&
    (!isSeries || !partialRequests || selected.size > 0)
  const qualityChoices =
    requestOptions.data?.destinations.flatMap((destination) =>
      destination.profiles.map((profile) => ({
        value: `${destination.id}:${profile.id}`,
        destination,
        profile,
      })),
    ) ?? []
  const defaultQuality =
    qualityChoices.find(
      (choice) => choice.destination.isDefault && choice.profile.isDefault,
    ) ??
    qualityChoices.find((choice) => choice.destination.isDefault) ??
    qualityChoices.find((choice) => choice.profile.isDefault) ??
    qualityChoices[0]
  const qualityValue = qualityChoices.some((choice) => choice.value === qualityChoice)
    ? qualityChoice!
    : defaultQuality?.value
  const selectedQuality = qualityChoices.find((choice) => choice.value === qualityValue)

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            Request {result.title}
            {result.year ? ` (${result.year})` : ""}
          </DialogTitle>
          <DialogDescription>
            {selectedCapability?.autoApprove
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
            is4k={is4k}
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
            {isSeries
              ? `Seerr already has every ${is4k ? "4K " : ""}season of this series.`
              : `This ${is4k ? "4K " : ""}version is already in Seerr.`}
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
              onChange={(event) => {
                setIs4k(event.target.checked)
                setChosen(null)
                setQualityChoice(null)
              }}
            />
            Request in 4K
          </Label>
        )}

        {advancedRequest && qualityChoices.length > 0 && (
          <div className="space-y-2">
            <Label>Download quality</Label>
            <Select value={qualityValue} onValueChange={setQualityChoice}>
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Use Seerr’s default profile" />
              </SelectTrigger>
              <SelectContent>
                {requestOptions.data?.destinations.map((destination) => (
                  <SelectGroup key={destination.id}>
                    <SelectLabel>
                      {destination.name}
                      {destination.isDefault ? " · default destination" : ""}
                    </SelectLabel>
                    {destination.profiles.map((profile) => (
                      <SelectItem
                        key={`${destination.id}:${profile.id}`}
                        value={`${destination.id}:${profile.id}`}
                      >
                        {profile.name}
                        {profile.isDefault ? " · default" : ""}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              Seerr sends this to {isSeries ? "Sonarr" : "Radarr"} and keeps the
              destination’s configured root folder.
            </p>
          </div>
        )}
        {qualityLoading && <Skeleton className="h-16" />}
        {advancedRequest && requestOptions.error && (
          <p className="text-xs text-muted-foreground">
            Quality profiles could not be loaded; Seerr will use its configured default.
          </p>
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
                  serverId: selectedQuality?.destination.id,
                  profileId: selectedQuality?.profile.id,
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
