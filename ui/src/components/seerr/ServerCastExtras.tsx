import { MediaCard } from "@/components/MediaCard"
import type { CastCreditsInput } from "@/components/seerr/use-cast-credits"
import { useCastCredits } from "@/components/seerr/use-cast-credits"

export interface ServerCastExtrasProps extends CastCreditsInput {
  personName: string
}

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
      aria-label={`More server titles with ${personName}`}
    >
      <div>
        <h2 className="section-title">More on your Jellyfin server</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          Titles with {personName} that your server has, found through their complete credits.
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
