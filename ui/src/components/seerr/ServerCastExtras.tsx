import { MediaCard } from "@/components/MediaCard"
import type { CastCreditsInput } from "@/components/seerr/use-cast-credits"
import { useCastCredits } from "@/components/seerr/use-cast-credits"

export interface ServerCastExtrasProps extends CastCreditsInput {
  personName: string
}

/**
 * The server titles a cast member is provably in even though Jellyfin's stored
 * cast list never names them there. The backend only returns titles it verified
 * against the live server by TMDB id, so everything listed here sits in the
 * library; rendering stays silent until at least one exists.
 */
export function ServerCastExtras({
  personName,
  jellyfinId,
  tmdbId,
  resolving = false,
  resolutionError = null,
}: ServerCastExtrasProps) {
  const { credits, phase } = useCastCredits({
    jellyfinId,
    tmdbId,
    resolving,
    resolutionError,
  })
  const extras = credits.data?.libraryExtras ?? []
  if (phase !== "ready" || !extras.length) return null

  return (
    <section
      className="flex flex-col gap-4 pt-8"
      aria-label={`More server titles featuring ${personName}`}
    >
      <div>
        <h2 className="section-title">More on your Jellyfin server</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          Titles featuring {personName} that your server has, found through their complete cast credits.
        </p>
      </div>
      <div className="flex flex-wrap gap-[var(--card-gap)]">
        {extras.map((item) => (
          <MediaCard key={item.id} item={item} className="catalog-card" />
        ))}
      </div>
    </section>
  )
}
