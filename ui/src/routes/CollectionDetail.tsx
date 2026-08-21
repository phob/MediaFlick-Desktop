import { ArrowLeft, Layers } from "lucide-react"
import { Link, useParams } from "react-router-dom"
import { PageErrorState } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import { SeerrResults } from "@/components/seerr/SeerrResults"
import { seerrImageUrl } from "@/lib/api"
import { useCollectionDetail } from "@/lib/queries"

/**
 * One TMDB collection: the movies the library already has sit beside the ones
 * it is missing, and the missing ones carry the usual discover/request flow.
 * Parts arrive in release order from TMDB, already joined to local ownership.
 */
export default function CollectionDetail() {
  const { id } = useParams<{ id: string }>()
  const parsedId = Number(id)
  const validId = Number.isSafeInteger(parsedId) && parsedId > 0 ? parsedId : null
  const { data, isPending, error, refetch } = useCollectionDetail(validId)

  if (!validId) {
    return (
      <div className="p-6 sm:p-10 lg:p-14">
        <PageErrorState
          title="Invalid collection"
          description="That address does not identify a TMDB collection."
        />
      </div>
    )
  }
  if (error && !data) {
    return (
      <div className="p-6 sm:p-10 lg:p-14">
        <PageErrorState
          title="Could not load collection"
          description={error.message}
          action={
            <Button variant="outline" onClick={() => void refetch()}>
              Try again
            </Button>
          }
        />
      </div>
    )
  }

  const owned = data?.parts.filter((part) => part.libraryItemId).length ?? 0
  const missing = (data?.parts.length ?? 0) - owned
  const backdrop = seerrImageUrl(data?.backdropPath, "w1280")
  const poster = seerrImageUrl(data?.posterPath, "w342")

  return (
    <div className="flex min-w-0 flex-col gap-8 pb-16">
      <header className="relative overflow-hidden">
        {backdrop && (
          <div className="pointer-events-none absolute inset-0" aria-hidden>
            <img
              src={backdrop}
              alt=""
              decoding="async"
              className="media-backdrop-image h-full w-full object-cover opacity-25"
            />
            <div className="absolute inset-0 bg-linear-to-t from-background via-background/70 to-background/30" />
          </div>
        )}
        <div className="relative z-10 flex items-end gap-6 px-6 pt-10 sm:px-10 lg:px-14">
          <div className="hidden h-40 w-27 shrink-0 overflow-hidden rounded-lg bg-card shadow-xl ring-1 ring-white/10 sm:block">
            {poster ? (
              <img src={poster} alt="" decoding="async" className="media-artwork-image h-full w-full object-cover" />
            ) : (
              <div className="flex h-full w-full items-center justify-center text-muted-foreground">
                <Layers className="size-7" aria-hidden />
              </div>
            )}
          </div>
          <div className="min-w-0 flex-1 pb-1">
            <Link
              to="/collections"
              className="inline-flex items-center gap-1 text-sm text-muted-foreground transition hover:text-foreground"
            >
              <ArrowLeft className="size-3.5" />
              Collections
            </Link>
            <h1 className="mt-1 truncate text-2xl font-semibold tracking-tight">
              {data?.name ?? "…"}
            </h1>
            {data?.parts.length ? (
              <p className="data-value mt-1 text-muted-foreground">
                {owned} of {data.parts.length} in your library
                {missing > 0 && ` · ${missing} missing`}
              </p>
            ) : null}
          </div>
        </div>
        {data?.overview && (
          <p className="relative z-10 mt-4 max-w-3xl px-6 text-sm leading-relaxed text-foreground/85 sm:px-10 lg:px-14">
            {data.overview}
          </p>
        )}
      </header>

      <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
        <h2 className="section-title">Movies</h2>
        <SeerrResults
          results={data?.parts}
          isPending={isPending}
          error={error}
          empty="TMDB lists no movies in this collection."
        />
      </section>
    </div>
  )
}
